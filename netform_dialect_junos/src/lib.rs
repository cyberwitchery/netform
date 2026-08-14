//! junos-oriented dialect profile for `netform_ir`.
//!
//! this crate provides a conservative Junos profile that customizes:
//! - comment classification (`#`, `/*`, `*`, `*/`)
//! - line tokenization for braces/semicolons and quoted strings
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_junos::parse_junos;
//!
//! let cfg = "interfaces {\n    ge-0/0/0 {\n        disable;\n    }\n}\n";
//! let doc = parse_junos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Dialect, DialectHint, Document, LiteralTerminator, ParsedLineParts, TriviaKind,
    classify_trivia_with_prefixes, ends_inside_quoted_value, parse_with_dialect, tokenize,
};

/// dialect implementation for Junos-like configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct JunosDialect;

/// parse text using [`JunosDialect`].
pub fn parse_junos(input: &str) -> Document {
    parse_with_dialect(input, &JunosDialect)
}

impl Dialect for JunosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("junos".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_junos_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_junos_parts(raw)
    }

    fn key_hint(
        &self,
        _raw: &str,
        parsed: Option<&ParsedLineParts>,
        trivia: TriviaKind,
    ) -> Option<String> {
        if trivia != TriviaKind::Content {
            return None;
        }
        junos_key_hint(parsed)
    }

    fn block_terminator(&self, raw: &str) -> bool {
        matches!(raw.trim(), "}" | "};")
    }

    fn literal_region(&self, raw: &str) -> Option<LiteralTerminator> {
        junos_literal_region(raw)
    }
}

/// recognize a Junos line whose double-quoted value is still open at the end of
/// the line, and report what closes it.
///
/// certificates, ssh keys and login banners are emitted as a quoted value
/// spanning many physical lines, in both the braced and the `set` form; the
/// value ends at the next unescaped double quote, wherever on its line that
/// falls.  a self-contained value (`description "uplink";`) and a comment line
/// open no region.
///
/// # Example
///
/// ```rust
/// use netform_dialect_junos::junos_literal_region;
///
/// let region = junos_literal_region("    certificate \"-----BEGIN CERTIFICATE-----").unwrap();
/// assert!(region.terminates("-----END CERTIFICATE-----\";"));
/// assert!(junos_literal_region("    description \"uplink\";").is_none());
/// ```
pub fn junos_literal_region(raw: &str) -> Option<LiteralTerminator> {
    if classify_junos_trivia(raw) != TriviaKind::Content {
        return None;
    }

    ends_inside_quoted_value(raw).then_some(LiteralTerminator::UnescapedQuote)
}

fn classify_junos_trivia(raw: &str) -> TriviaKind {
    classify_trivia_with_prefixes(raw, &["#", "/*", "* ", "*/"])
}

fn parse_junos_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, &['{', '}', ';']);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

fn junos_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.iter().map(String::as_str).collect::<Vec<_>>();

    match head {
        "interfaces" | "protocols" | "routing-instances" | "policy-options" | "firewall"
        | "security" | "snmp" | "vlans" | "chassis" | "class-of-service" | "forwarding-options"
        | "applications" | "groups" | "system" | "routing-options" => Some(head.to_string()),
        "set" => set_style_key_hint(&args),
        _ => None,
    }
}

fn set_style_key_hint(args: &[&str]) -> Option<String> {
    match args {
        ["interfaces", name, ..] => Some(format!("set-interface:{name}")),
        ["routing-instances", name, ..] => Some(format!("set-routing-instance:{name}")),
        ["protocols", "bgp", asn, ..] => Some(format!("set-protocols:bgp:{asn}")),
        ["protocols", proto, ..] => Some(format!("set-protocols:{proto}")),
        ["firewall", kind @ ("filter" | "policer"), name, ..] => {
            Some(format!("set-firewall:{kind}:{name}"))
        }
        ["vlans", name, ..] => Some(format!("set-vlan:{name}")),
        ["applications", kind, name, ..] => Some(format!("set-applications:{kind}:{name}")),
        ["groups", name, ..] => Some(format!("set-group:{name}")),
        ["policy-options", kind, name, ..] => Some(format!("set-policy-options:{kind}:{name}")),
        [
            section @ ("system" | "security" | "snmp" | "chassis" | "class-of-service"
            | "forwarding-options" | "routing-options"),
            rest @ ..,
        ] => set_identity_len(section, rest)
            .and_then(|len| rest.get(..len))
            .map(|identity| set_hint(section, identity)),
        _ => None,
    }
}

/// number of leading tokens in a `set` statement's section-relative arguments
/// that form its identity; the remaining tokens are its value.
///
/// `None` means the statement keys on its full text instead.  That is the
/// default because a Junos leaf just as often ends in a set member
/// (`... system-services ssh`) as in a value, and truncating a member would
/// merge distinct statements onto one key.
///
/// a returned length can drop a trailing value (`next-hop <addr>`, bare
/// `qualified-next-hop <addr>`); statements differing only in that value
/// collide on one key, accepted so a value change diffs as an edit rather
/// than an add/remove pair.
fn set_identity_len(section: &str, args: &[&str]) -> Option<usize> {
    match (section, args) {
        ("system", ["host-name" | "domain-name" | "time-zone", _]) => Some(1),
        ("system", ["location", _, _]) => Some(2),
        ("system", ["root-authentication", "encrypted-password", _]) => Some(2),
        (
            "system",
            [
                "services",
                "ssh",
                "ciphers" | "macs" | "key-exchange" | "hostkey-algorithm",
                _,
            ],
        ) => None,
        ("system", ["services", _, _, _]) => Some(3),
        (
            "system",
            [
                "login",
                "user",
                _,
                "authentication",
                "encrypted-password",
                _,
            ],
        ) => Some(5),
        ("system", ["login", "user", _, "authentication", ..]) => None,
        ("system", ["login", "user", _, _, ..]) => Some(4),
        ("system", ["syslog", "host" | "file" | "user", _, _, _]) => Some(4),

        ("security", ["policies", "default-policy", _]) => Some(2),
        ("security", ["zones", "security-zone", _, "description" | "screen", _]) => Some(4),
        ("security", ["address-book", _, "address", _, _]) => Some(4),

        ("snmp", ["contact" | "location" | "name", _]) => Some(1),
        ("snmp", ["community", _, "authorization", _]) => Some(3),

        ("chassis", ["alarm", _, _, _]) => Some(3),
        ("chassis", ["aggregated-devices", _, "device-count", _]) => Some(3),

        ("class-of-service", ["interfaces", _, "scheduler-map", _]) => Some(3),
        ("class-of-service", ["interfaces", _, "unit", _, "scheduler-map", _]) => Some(5),

        ("forwarding-options", ["sampling", "input", _, _]) => Some(3),

        ("routing-options", ["autonomous-system" | "router-id", _]) => Some(1),
        (
            "routing-options",
            [
                "static" | "aggregate" | "generate",
                "route",
                _,
                "discard" | "reject" | "receive",
            ],
        ) => Some(3),
        (
            "routing-options",
            [
                "static" | "aggregate" | "generate",
                "route",
                _,
                "qualified-next-hop",
                _,
                _,
                ..,
            ],
        ) => Some(6),
        ("routing-options", ["static" | "aggregate" | "generate", "route", _, _, ..]) => Some(4),
        (
            "routing-options",
            [
                "rib",
                _,
                "static" | "aggregate" | "generate",
                "route",
                _,
                "discard" | "reject" | "receive",
            ],
        ) => Some(5),
        (
            "routing-options",
            [
                "rib",
                _,
                "static" | "aggregate" | "generate",
                "route",
                _,
                "qualified-next-hop",
                _,
                _,
                ..,
            ],
        ) => Some(8),
        (
            "routing-options",
            [
                "rib",
                _,
                "static" | "aggregate" | "generate",
                "route",
                _,
                _,
                ..,
            ],
        ) => Some(6),

        _ => None,
    }
}

fn set_hint(section: &str, identity: &[&str]) -> String {
    let mut hint = format!("set-{section}");
    for token in identity {
        hint.push(':');
        hint.push_str(token);
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junos_comment_classification_supports_hash_and_block_styles() {
        assert_eq!(classify_junos_trivia("# note"), TriviaKind::Comment);
        assert_eq!(classify_junos_trivia("/* note */"), TriviaKind::Comment);
        assert_eq!(classify_junos_trivia("*/"), TriviaKind::Comment);
        assert_eq!(
            classify_junos_trivia("  * continuation"),
            TriviaKind::Comment
        );
        assert_eq!(classify_junos_trivia("interfaces {"), TriviaKind::Content);
    }

    #[test]
    fn star_without_trailing_space_is_not_a_comment() {
        assert_eq!(classify_junos_trivia("*10.0.0.0/8"), TriviaKind::Content);
        assert_eq!(classify_junos_trivia("*[BGP/170]"), TriviaKind::Content);
    }

    #[test]
    fn literal_region_opens_on_an_unclosed_quoted_value() {
        assert_eq!(
            junos_literal_region("                certificate \"-----BEGIN CERTIFICATE-----"),
            Some(LiteralTerminator::UnescapedQuote),
        );
        assert_eq!(
            junos_literal_region("        ssh-rsa \"ssh-rsa AAAAB3Nz"),
            Some(LiteralTerminator::UnescapedQuote),
        );
        assert_eq!(
            junos_literal_region("set system login announcement \"Authorized use only"),
            Some(LiteralTerminator::UnescapedQuote),
        );
    }

    #[test]
    fn literal_region_declines_self_contained_values() {
        assert_eq!(junos_literal_region("    description \"uplink\";"), None);
        assert_eq!(junos_literal_region("set snmp location \"rack 4\""), None);
        assert_eq!(junos_literal_region("interfaces {"), None);
        assert_eq!(junos_literal_region("    }"), None);
        assert_eq!(junos_literal_region("};"), None);
        assert_eq!(junos_literal_region(""), None);
    }

    #[test]
    fn literal_region_declines_unquoted_junos_statements() {
        assert_eq!(junos_literal_region("    apply-groups GRP-DEFAULTS;"), None);
        assert_eq!(junos_literal_region("    apply-groups-except RE1;"), None);
        assert_eq!(
            junos_literal_region("annotate interfaces \"managed by netops\""),
            None,
        );
        assert_eq!(junos_literal_region("    inactive: disable;"), None);
        assert_eq!(
            junos_literal_region("set protocols bgp group PEERS neighbor 10.0.0.1"),
            None,
        );
    }

    #[test]
    fn literal_region_opens_on_an_escaped_quote_in_the_opener() {
        assert_eq!(
            junos_literal_region(r#"    message "say \"hello\" first"#),
            Some(LiteralTerminator::UnescapedQuote),
        );
        assert_eq!(
            junos_literal_region(r#"    message "say \"hello\" first";"#),
            None,
        );
    }

    #[test]
    fn literal_region_declines_a_comment_holding_an_odd_quote() {
        assert_eq!(junos_literal_region("## Last changed by \"admin"), None);
        assert_eq!(junos_literal_region("# unbalanced \" here"), None);
        assert_eq!(junos_literal_region("/* he said \" and left"), None);
        assert_eq!(junos_literal_region(" * still a \" comment"), None);
        assert_eq!(junos_literal_region("*/ trailing \""), None);
    }

    #[test]
    fn junos_tokenization_keeps_brace_and_semicolon_tokens() {
        let parsed = parse_junos_parts("interfaces {").expect("content should parse");
        assert_eq!(parsed.head, "interfaces");
        assert_eq!(parsed.args, vec!["{"]);

        let parsed =
            parse_junos_parts("description \"Uplink to core\";").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Uplink to core\"", ";"]);
    }

    #[test]
    fn parse_junos_sets_named_dialect_hint() {
        let doc = parse_junos("set system host-name router-1\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("junos".into())
        );
    }

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_junos_parts(line);
        junos_key_hint(parsed.as_ref())
    }

    /// every distinct statement must land on a distinct key.
    fn assert_all_distinct(lines: &[&str]) {
        let mut seen: Vec<(&str, String)> = Vec::new();
        for line in lines {
            let key = hint(line).unwrap_or_else(|| line.to_string());
            if let Some((other, _)) = seen.iter().find(|(_, k)| *k == key) {
                panic!("`{line}` and `{other}` collide on key `{key}`");
            }
            seen.push((line, key));
        }
    }

    #[test]
    fn key_hint_interfaces() {
        assert_eq!(hint("interfaces {"), Some("interfaces".into()));
    }

    #[test]
    fn key_hint_protocols() {
        assert_eq!(hint("protocols {"), Some("protocols".into()));
    }

    #[test]
    fn key_hint_routing_instances() {
        assert_eq!(
            hint("routing-instances {"),
            Some("routing-instances".into())
        );
    }

    #[test]
    fn key_hint_policy_options() {
        assert_eq!(hint("policy-options {"), Some("policy-options".into()));
    }

    #[test]
    fn key_hint_firewall() {
        assert_eq!(hint("firewall {"), Some("firewall".into()));
    }

    #[test]
    fn key_hint_security() {
        assert_eq!(hint("security {"), Some("security".into()));
    }

    #[test]
    fn key_hint_snmp() {
        assert_eq!(hint("snmp {"), Some("snmp".into()));
    }

    #[test]
    fn key_hint_vlans() {
        assert_eq!(hint("vlans {"), Some("vlans".into()));
    }

    #[test]
    fn key_hint_chassis() {
        assert_eq!(hint("chassis {"), Some("chassis".into()));
    }

    #[test]
    fn key_hint_class_of_service() {
        assert_eq!(hint("class-of-service {"), Some("class-of-service".into()));
    }

    #[test]
    fn key_hint_forwarding_options() {
        assert_eq!(
            hint("forwarding-options {"),
            Some("forwarding-options".into()),
        );
    }

    #[test]
    fn key_hint_applications() {
        assert_eq!(hint("applications {"), Some("applications".into()));
    }

    #[test]
    fn key_hint_groups() {
        assert_eq!(hint("groups {"), Some("groups".into()));
    }

    #[test]
    fn key_hint_set_interface() {
        assert_eq!(
            hint("set interfaces ge-0/0/0 unit 0 family inet"),
            Some("set-interface:ge-0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_instance() {
        assert_eq!(
            hint("set routing-instances VRF1 instance-type vrf"),
            Some("set-routing-instance:VRF1".into()),
        );
    }

    #[test]
    fn key_hint_set_protocols_bgp() {
        assert_eq!(
            hint("set protocols bgp 65001 group PEERS"),
            Some("set-protocols:bgp:65001".into()),
        );
    }

    #[test]
    fn key_hint_set_protocols_ospf() {
        assert_eq!(
            hint("set protocols ospf area 0.0.0.0 interface ge-0/0/0.0"),
            Some("set-protocols:ospf".into()),
        );
    }

    #[test]
    fn key_hint_set_protocols_isis() {
        assert_eq!(
            hint("set protocols isis interface ge-0/0/0.0"),
            Some("set-protocols:isis".into()),
        );
    }

    #[test]
    fn key_hint_set_firewall_filter() {
        assert_eq!(
            hint("set firewall filter PROTECT-RE term 10 from protocol tcp"),
            Some("set-firewall:filter:PROTECT-RE".into()),
        );
    }

    #[test]
    fn key_hint_set_firewall_policer() {
        assert_eq!(
            hint("set firewall policer RATE-LIMIT if-exceeding bandwidth-limit 1m"),
            Some("set-firewall:policer:RATE-LIMIT".into()),
        );
    }

    #[test]
    fn key_hint_set_security_zone_description() {
        assert_eq!(
            hint("set security zones security-zone TRUST description \"trusted\""),
            Some("set-security:zones:security-zone:TRUST:description".into()),
        );
    }

    #[test]
    fn key_hint_set_security_address_book() {
        assert_eq!(
            hint("set security address-book global address WEB 10.0.0.1/32"),
            Some("set-security:address-book:global:address:WEB".into()),
        );
    }

    #[test]
    fn key_hint_set_security_default_policy() {
        assert_eq!(
            hint("set security policies default-policy deny-all"),
            Some("set-security:policies:default-policy".into()),
        );
    }

    #[test]
    fn key_hint_set_security_membership_leaves_key_on_full_text() {
        assert_eq!(
            hint(
                "set security zones security-zone TRUST host-inbound-traffic system-services dhcp"
            ),
            None,
        );
        assert_eq!(
            hint("set security nat source rule-set NAT-OUT from zone TRUST"),
            None,
        );
    }

    #[test]
    fn key_hint_set_vlans() {
        assert_eq!(
            hint("set vlans VLAN100 vlan-id 100"),
            Some("set-vlan:VLAN100".into()),
        );
    }

    #[test]
    fn key_hint_set_applications() {
        assert_eq!(
            hint("set applications application HTTP protocol tcp destination-port 80"),
            Some("set-applications:application:HTTP".into()),
        );
    }

    #[test]
    fn key_hint_set_applications_set() {
        assert_eq!(
            hint("set applications application-set WEB-APPS application HTTP"),
            Some("set-applications:application-set:WEB-APPS".into()),
        );
    }

    #[test]
    fn key_hint_set_groups() {
        assert_eq!(
            hint("set groups GRP-DEFAULTS system services ssh"),
            Some("set-group:GRP-DEFAULTS".into()),
        );
    }

    #[test]
    fn key_hint_set_policy_options_policy_statement() {
        assert_eq!(
            hint("set policy-options policy-statement EXPORT-BGP term 10 then accept"),
            Some("set-policy-options:policy-statement:EXPORT-BGP".into()),
        );
    }

    #[test]
    fn key_hint_set_policy_options_prefix_list() {
        assert_eq!(
            hint("set policy-options prefix-list INTERNAL 10.0.0.0/8"),
            Some("set-policy-options:prefix-list:INTERNAL".into()),
        );
    }

    #[test]
    fn key_hint_set_policy_options_community() {
        assert_eq!(
            hint("set policy-options community CUST-A members 65001:100"),
            Some("set-policy-options:community:CUST-A".into()),
        );
    }

    #[test]
    fn key_hint_set_snmp() {
        assert_eq!(
            hint("set snmp community public authorization read-only"),
            Some("set-snmp:community:public:authorization".into()),
        );
        assert_eq!(
            hint("set snmp location \"rack 4\""),
            Some("set-snmp:location".into()),
        );
    }

    #[test]
    fn key_hint_set_snmp_distinguishes_communities() {
        assert_all_distinct(&[
            "set snmp community public authorization read-only",
            "set snmp community private authorization read-write",
            "set snmp location \"rack 4\"",
            "set snmp contact ops@example.com",
        ]);
    }

    #[test]
    fn key_hint_set_chassis() {
        assert_eq!(
            hint("set chassis alarm management-ethernet link-down ignore"),
            Some("set-chassis:alarm:management-ethernet:link-down".into()),
        );
        assert_eq!(
            hint("set chassis aggregated-devices ethernet device-count 8"),
            Some("set-chassis:aggregated-devices:ethernet:device-count".into()),
        );
    }

    #[test]
    fn key_hint_set_class_of_service() {
        assert_eq!(
            hint("set class-of-service interfaces ge-0/0/0 scheduler-map SCHED"),
            Some("set-class-of-service:interfaces:ge-0/0/0:scheduler-map".into()),
        );
        assert_eq!(
            hint("set class-of-service interfaces ge-0/0/0 unit 0 scheduler-map SCHED"),
            Some("set-class-of-service:interfaces:ge-0/0/0:unit:0:scheduler-map".into()),
        );
    }

    #[test]
    fn key_hint_set_class_of_service_distinguishes_interfaces() {
        assert_all_distinct(&[
            "set class-of-service interfaces ge-0/0/0 scheduler-map SCHED-A",
            "set class-of-service interfaces ge-0/0/1 scheduler-map SCHED-B",
            "set class-of-service interfaces ge-0/0/0 unit 0 scheduler-map SCHED-C",
        ]);
    }

    #[test]
    fn key_hint_set_forwarding_options() {
        assert_eq!(
            hint("set forwarding-options sampling input rate 1000"),
            Some("set-forwarding-options:sampling:input:rate".into()),
        );
    }

    #[test]
    fn key_hint_system() {
        assert_eq!(hint("system {"), Some("system".into()));
    }

    #[test]
    fn key_hint_routing_options() {
        assert_eq!(hint("routing-options {"), Some("routing-options".into()));
    }

    #[test]
    fn key_hint_set_system_host_name() {
        assert_eq!(
            hint("set system host-name router-1"),
            Some("set-system:host-name".into()),
        );
    }

    #[test]
    fn key_hint_set_system_domain_name() {
        assert_eq!(
            hint("set system domain-name example.com"),
            Some("set-system:domain-name".into()),
        );
    }

    #[test]
    fn key_hint_set_system_time_zone() {
        assert_eq!(
            hint("set system time-zone UTC"),
            Some("set-system:time-zone".into()),
        );
    }

    #[test]
    fn key_hint_set_system_location() {
        assert_eq!(
            hint("set system location building \"HQ\""),
            Some("set-system:location:building".into()),
        );
    }

    #[test]
    fn key_hint_set_system_root_authentication() {
        assert_eq!(
            hint("set system root-authentication encrypted-password \"$6$abc\""),
            Some("set-system:root-authentication:encrypted-password".into()),
        );
    }

    #[test]
    fn key_hint_set_system_root_ssh_keys_key_on_full_text() {
        for kind in ["ssh-rsa", "ssh-ed25519", "ssh-ecdsa", "ssh-dss"] {
            assert_eq!(
                hint(&format!(
                    "set system root-authentication {kind} \"AAAAB3NzaC1yc2E\""
                )),
                None,
                "a root stanza carries several `{kind}` keys",
            );
        }
    }

    #[test]
    fn key_hint_set_system_root_authentication_distinguishes_entries() {
        assert_all_distinct(&[
            "set system root-authentication encrypted-password \"$6$abc\"",
            "set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"",
            "set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"",
            "set system root-authentication ssh-ed25519 \"AAAAC3NzaC1lZDI\"",
        ]);
    }

    #[test]
    fn key_hint_set_system_services() {
        assert_eq!(
            hint("set system services ssh root-login deny"),
            Some("set-system:services:ssh:root-login".into()),
        );
    }

    #[test]
    fn key_hint_set_system_services_multi_value_leaves_key_on_full_text() {
        assert_eq!(hint("set system services ssh ciphers aes256-ctr"), None);
        assert_eq!(hint("set system services ssh macs hmac-sha2-256"), None);
        assert_eq!(
            hint("set system services ssh key-exchange ecdh-sha2-nistp256"),
            None,
        );
        assert_eq!(
            hint("set system services ssh hostkey-algorithm ssh-ed25519"),
            None,
        );
    }

    #[test]
    fn key_hint_set_system_login() {
        assert_eq!(
            hint("set system login user admin class super-user"),
            Some("set-system:login:user:admin:class".into()),
        );
        assert_eq!(
            hint("set system login user admin authentication encrypted-password \"$6$abc\""),
            Some("set-system:login:user:admin:authentication:encrypted-password".into()),
        );
    }

    #[test]
    fn key_hint_set_system_login_ssh_keys_key_on_full_text() {
        assert_eq!(
            hint("set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E\""),
            None,
        );
        assert_eq!(
            hint("set system login user admin authentication ssh-ed25519 \"AAAAC3NzaC1lZDI\""),
            None,
        );
    }

    #[test]
    fn key_hint_set_system_syslog() {
        assert_eq!(
            hint("set system syslog host 10.0.0.2 any any"),
            Some("set-system:syslog:host:10.0.0.2:any".into()),
        );
        assert_eq!(
            hint("set system syslog file messages authorization info"),
            Some("set-system:syslog:file:messages:authorization".into()),
        );
    }

    #[test]
    fn key_hint_set_system_services_distinguishes_multi_value_leaves() {
        assert_all_distinct(&[
            "set system services ssh root-login deny",
            "set system services ssh ciphers aes256-ctr",
            "set system services ssh ciphers aes128-ctr",
            "set system services ssh macs hmac-sha2-256",
            "set system services ssh macs hmac-sha2-512",
            "set system services ssh key-exchange ecdh-sha2-nistp256",
            "set system services ssh key-exchange group14-sha1",
            "set system services ssh hostkey-algorithm ssh-rsa",
            "set system services ssh hostkey-algorithm ssh-ed25519",
        ]);
    }

    #[test]
    fn key_hint_set_system_login_distinguishes_authentication_entries() {
        assert_all_distinct(&[
            "set system login user admin class super-user",
            "set system login user admin authentication encrypted-password \"$6$abc\"",
            "set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"",
            "set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"",
            "set system login user admin authentication ssh-ed25519 \"AAAAC3NzaC1lZDI\"",
            "set system login user ops authentication encrypted-password \"$6$def\"",
        ]);
    }

    #[test]
    fn key_hint_set_system_membership_leaves_key_on_full_text() {
        assert_eq!(hint("set system name-server 8.8.8.8"), None);
        assert_eq!(hint("set system ntp server 10.0.0.1"), None);
        assert_eq!(hint("set system authentication-order radius"), None);
        assert_eq!(hint("set system no-redirects"), None);
    }

    #[test]
    fn key_hint_set_system_distinguishes_statements() {
        assert_all_distinct(&[
            "set system host-name router-1",
            "set system domain-name example.com",
            "set system time-zone UTC",
            "set system name-server 8.8.8.8",
            "set system name-server 1.1.1.1",
            "set system services ssh root-login deny",
            "set system services ssh protocol-version v2",
            "set system services netconf ssh",
            "set system login user admin class super-user",
            "set system login user admin uid 2000",
            "set system login user ops class read-only",
            "set system syslog host 10.0.0.2 any any",
            "set system syslog host 10.0.0.2 authorization info",
            "set system syslog host 10.0.0.3 any any",
            "set system ntp server 10.0.0.1",
            "set system ntp server 10.0.0.2",
        ]);
    }

    #[test]
    fn key_hint_set_routing_options_static_route_keys_on_prefix_and_attribute() {
        assert_eq!(
            hint("set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1"),
            Some("set-routing-options:static:route:0.0.0.0/0:next-hop".into()),
        );
        assert_eq!(
            hint("set routing-options static route 0.0.0.0/0 preference 5"),
            Some("set-routing-options:static:route:0.0.0.0/0:preference".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options_next_hop_type_flag_keys_on_prefix() {
        for flag in ["discard", "reject", "receive"] {
            assert_eq!(
                hint(&format!(
                    "set routing-options static route 10.0.0.0/8 {flag}"
                )),
                Some("set-routing-options:static:route:10.0.0.0/8".into()),
                "`{flag}` must key on the route so swapping one for another is a value change",
            );
        }
    }

    #[test]
    fn key_hint_set_routing_options_bare_route_keys_on_full_text() {
        assert_eq!(hint("set routing-options static route 10.0.0.0/8"), None);
        assert_eq!(
            hint("set routing-options rib inet6.0 static route ::/0"),
            None,
        );
    }

    #[test]
    fn key_hint_set_routing_options_rib_scoped_route() {
        assert_eq!(
            hint("set routing-options rib inet6.0 static route ::/0 next-hop 2001:db8::1"),
            Some("set-routing-options:rib:inet6.0:static:route:::/0:next-hop".into()),
        );
        assert_eq!(
            hint("set routing-options rib inet6.0 static route ::/0 discard"),
            Some("set-routing-options:rib:inet6.0:static:route:::/0".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options_aggregate_and_generate() {
        assert_eq!(
            hint("set routing-options aggregate route 10.0.0.0/8 policy AGG"),
            Some("set-routing-options:aggregate:route:10.0.0.0/8:policy".into()),
        );
        assert_eq!(
            hint("set routing-options generate route 0.0.0.0/0 policy GEN"),
            Some("set-routing-options:generate:route:0.0.0.0/0:policy".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options_autonomous_system() {
        assert_eq!(
            hint("set routing-options autonomous-system 65001"),
            Some("set-routing-options:autonomous-system".into()),
        );
        assert_eq!(
            hint("set routing-options router-id 10.0.0.1"),
            Some("set-routing-options:router-id".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options_distinguishes_statements() {
        assert_all_distinct(&[
            "set routing-options autonomous-system 65001",
            "set routing-options router-id 10.0.0.1",
            "set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1",
            "set routing-options static route 10.0.0.0/8 next-hop 10.0.0.2",
            "set routing-options rib inet6.0 static route ::/0 next-hop 2001:db8::1",
            "set routing-options forwarding-table export LOAD-BALANCE",
        ]);
    }

    #[test]
    fn key_hint_set_routing_options_distinguishes_route_attributes() {
        assert_all_distinct(&[
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1",
            "set routing-options static route 10.0.0.0/8 preference 5",
            "set routing-options static route 10.0.0.0/8 metric 10",
            "set routing-options static route 10.0.0.0/8 tag 100",
            "set routing-options static route 10.0.0.0/8 no-readvertise",
            "set routing-options static route 10.0.0.0/8 resolve",
            "set routing-options rib inet6.0 static route ::/0 next-hop 2001:db8::1",
            "set routing-options rib inet6.0 static route ::/0 preference 5",
        ]);
    }

    #[test]
    fn key_hint_set_routing_options_qualified_next_hop_keys_on_address_and_attribute() {
        assert_eq!(
            hint(
                "set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 preference 10"
            ),
            Some(
                "set-routing-options:static:route:10.0.0.0/8:qualified-next-hop:192.0.2.1:preference"
                    .into()
            ),
        );
        assert_eq!(
            hint(
                "set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 metric 5"
            ),
            Some(
                "set-routing-options:rib:inet6.0:static:route:::/0:qualified-next-hop:2001:db8::1:metric"
                    .into()
            ),
        );
    }

    #[test]
    fn key_hint_set_routing_options_bare_qualified_next_hop_keys_on_the_attribute() {
        assert_eq!(
            hint("set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1"),
            Some("set-routing-options:static:route:10.0.0.0/8:qualified-next-hop".into()),
            "a documented collision, as for `next-hop`",
        );
    }

    #[test]
    fn key_hint_set_routing_options_distinguishes_qualified_next_hop_attributes() {
        assert_all_distinct(&[
            "set routing-options static route 10.0.0.0/8 preference 5",
            "set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 preference 10",
            "set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 metric 5",
            "set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.2 preference 20",
            "set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 preference 10",
            "set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 metric 5",
        ]);
    }

    #[test]
    fn key_hint_set_routing_options_value_change_keeps_identity() {
        assert_eq!(
            hint("set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1"),
            hint("set routing-options static route 0.0.0.0/0 next-hop 10.0.0.9"),
        );
    }

    #[test]
    fn key_hint_none_for_unknown() {
        assert_eq!(hint("event-options {"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(junos_key_hint(None), None);
    }

    #[test]
    fn key_hint_set_unknown_section() {
        assert_eq!(hint("set event-options policy DUMP-ON-SNMPD"), None);
    }

    #[test]
    fn parse_junos_brace_round_trip() {
        let cfg = "\
interfaces {
    ge-0/0/0 {
        description \"uplink\";
    }
}
";
        let doc = parse_junos(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn closing_braces_attach_as_block_footers() {
        use netform_ir::Node;

        let cfg = "\
interfaces {
    ge-0/0/0 {
        disable;
    }
}
";
        let doc = parse_junos(cfg);
        assert_eq!(doc.render(), cfg, "round trip must stay byte-for-byte");

        // the outer `}` closes `interfaces` and lands in its footer.
        assert_eq!(doc.roots.len(), 1, "the terminator is no longer a sibling");
        let Some(Node::Block(interfaces)) = doc.node(doc.roots[0]) else {
            panic!("expected an interfaces block at the root");
        };
        assert_eq!(interfaces.header.raw, "interfaces {");
        assert_eq!(
            interfaces.footer.as_ref().map(|f| f.raw.as_str()),
            Some("}"),
        );

        // the inner `}` closes the nested `ge-0/0/0` block.
        assert_eq!(interfaces.children.len(), 1);
        let Some(Node::Block(intf)) = doc.node(interfaces.children[0]) else {
            panic!("expected a ge-0/0/0 block nested in interfaces");
        };
        assert_eq!(intf.header.raw, "    ge-0/0/0 {");
        assert_eq!(intf.footer.as_ref().map(|f| f.raw.as_str()), Some("    }"));
    }

    #[test]
    fn semicolon_brace_terminator_attaches_as_footer() {
        use netform_ir::Node;

        // some Junos stanzas close with `};`; it is treated as a terminator too.
        let cfg = "policy-options {\n    community NO-EXPORT members no-export;\n};\n";
        let doc = parse_junos(cfg);
        assert_eq!(doc.render(), cfg);
        let Some(Node::Block(block)) = doc.node(doc.roots[0]) else {
            panic!("expected a policy-options block");
        };
        assert_eq!(block.footer.as_ref().map(|f| f.raw.as_str()), Some("};"));
    }
}
