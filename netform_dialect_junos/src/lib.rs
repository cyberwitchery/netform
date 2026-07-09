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
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, classify_trivia_with_prefixes,
    parse_with_dialect, tokenize,
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
    let args = parsed.args.as_slice();

    match head {
        "interfaces" | "protocols" | "routing-instances" | "policy-options" | "firewall"
        | "security" | "snmp" | "vlans" | "chassis" | "class-of-service" | "forwarding-options"
        | "applications" | "groups" | "system" | "routing-options" => Some(head.to_string()),
        "set" => set_style_key_hint(args),
        _ => None,
    }
}

fn set_style_key_hint(args: &[String]) -> Option<String> {
    match args {
        [section, name, ..] if section == "interfaces" => Some(format!("set-interface:{name}")),
        [section, name, ..] if section == "routing-instances" => {
            Some(format!("set-routing-instance:{name}"))
        }
        [section, proto, asn, ..] if section == "protocols" && proto == "bgp" => {
            Some(format!("set-protocols:bgp:{asn}"))
        }
        [section, proto, ..] if section == "protocols" => Some(format!("set-protocols:{proto}")),
        [section, kind, name, ..]
            if section == "firewall" && (kind == "filter" || kind == "policer") =>
        {
            Some(format!("set-firewall:{kind}:{name}"))
        }
        [section, sub, _, name, ..] if section == "security" && sub == "zones" => {
            Some(format!("set-security:zone:{name}"))
        }
        [section, ..] if section == "security" => Some("set-security".into()),
        [section, name, ..] if section == "vlans" => Some(format!("set-vlan:{name}")),
        [section, kind, name, ..] if section == "applications" => {
            Some(format!("set-applications:{kind}:{name}"))
        }
        [section, name, ..] if section == "groups" => Some(format!("set-group:{name}")),
        [section, kind, name, ..] if section == "policy-options" => {
            Some(format!("set-policy-options:{kind}:{name}"))
        }
        [section, sub, ..]
            if section == "system"
                && matches!(
                    sub.as_str(),
                    "host-name" | "services" | "login" | "ntp" | "syslog"
                ) =>
        {
            Some(format!("set-system:{sub}"))
        }
        [section, ..] if section == "system" => Some("set-system".into()),
        [section, ..]
            if matches!(
                section.as_str(),
                "snmp" | "chassis" | "class-of-service" | "forwarding-options" | "routing-options"
            ) =>
        {
            Some(format!("set-{section}"))
        }
        _ => None,
    }
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
    fn key_hint_set_security_zone() {
        assert_eq!(
            hint(
                "set security zones security-zone TRUST host-inbound-traffic system-services dhcp"
            ),
            Some("set-security:zone:TRUST".into()),
        );
    }

    #[test]
    fn key_hint_set_security_fallback() {
        assert_eq!(
            hint("set security nat source rule-set NAT-OUT from zone TRUST"),
            Some("set-security".into()),
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
            Some("set-snmp".into()),
        );
    }

    #[test]
    fn key_hint_set_chassis() {
        assert_eq!(
            hint("set chassis alarm management-ethernet link-down ignore"),
            Some("set-chassis".into()),
        );
    }

    #[test]
    fn key_hint_set_class_of_service() {
        assert_eq!(
            hint("set class-of-service interfaces ge-0/0/0 scheduler-map SCHED"),
            Some("set-class-of-service".into()),
        );
    }

    #[test]
    fn key_hint_set_forwarding_options() {
        assert_eq!(
            hint("set forwarding-options sampling input rate 1000"),
            Some("set-forwarding-options".into()),
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
    fn key_hint_set_system_services() {
        assert_eq!(
            hint("set system services ssh root-login deny"),
            Some("set-system:services".into()),
        );
    }

    #[test]
    fn key_hint_set_system_login() {
        assert_eq!(
            hint("set system login user admin class super-user"),
            Some("set-system:login".into()),
        );
    }

    #[test]
    fn key_hint_set_system_ntp() {
        assert_eq!(
            hint("set system ntp server 10.0.0.1"),
            Some("set-system:ntp".into()),
        );
    }

    #[test]
    fn key_hint_set_system_syslog() {
        assert_eq!(
            hint("set system syslog host 10.0.0.2 any any"),
            Some("set-system:syslog".into()),
        );
    }

    #[test]
    fn key_hint_set_system_fallback() {
        assert_eq!(
            hint("set system name-server 8.8.8.8"),
            Some("set-system".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options() {
        assert_eq!(
            hint("set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1"),
            Some("set-routing-options".into()),
        );
    }

    #[test]
    fn key_hint_set_routing_options_autonomous_system() {
        assert_eq!(
            hint("set routing-options autonomous-system 65001"),
            Some("set-routing-options".into()),
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
