//! the IOS-family dialect shared by EOS, IOS XE, IOS XR and NX-OS.

use crate::IosRules;
use crate::rules::{KeyRule, KeyRuleAction};
use netform_ir::{
    Dialect, DialectHint, LiteralTerminator, ParsedLineParts, TriviaKind,
    classify_trivia_with_prefixes, parse_parts_with_punctuation,
};

/// parameterized dialect for IOS-like configuration text (EOS, IOS XE, NX-OS, …).
///
/// all IOS-like dialects share the same trivia classification and line
/// tokenization; they differ only in the hint name stored in
/// [`DocumentMetadata`](netform_ir::DocumentMetadata) and the key-hint derivation function.  construct with
/// [`IosLikeDialect::new`], passing the dialect name and its key-hint function:
///
/// ```rust
/// use netform_dialects::ios_like::IosLikeDialect;
/// use netform_ir::parse_with_dialect;
///
/// let dialect = IosLikeDialect::new("iosxe", |parsed| {
///     netform_dialects::find("iosxe")?.rules?.key_hint(parsed)
/// });
/// let doc = parse_with_dialect("hostname edge-1\n", &dialect);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct IosLikeDialect {
    name: &'static str,
    key_hint: fn(Option<&ParsedLineParts>) -> Option<String>,
    block_terminator: Option<fn(&str) -> bool>,
    empty_block: Option<fn(&str, &str) -> bool>,
}

impl IosLikeDialect {
    /// create a dialect instance tagged with the given hint name and key-hint
    /// derivation function.
    ///
    /// `key_hint` maps a parsed content line to its stable identity hint (or
    /// `None`); each IOS-family vendor module passes its own so interface-type
    /// normalization and vendor-specific constructs stay local to that module.
    ///
    /// the dialect reports no [`Dialect::block_terminator`] and no
    /// [`Dialect::empty_block`]; add them with
    /// [`IosLikeDialect::with_block_terminator`] and
    /// [`IosLikeDialect::with_empty_block`].
    pub const fn new(
        name: &'static str,
        key_hint: fn(Option<&ParsedLineParts>) -> Option<String>,
    ) -> Self {
        Self {
            name,
            key_hint,
            block_terminator: None,
            empty_block: None,
        }
    }

    /// return this dialect with `is_terminator` answering
    /// [`Dialect::block_terminator`].
    ///
    /// ```rust
    /// use netform_dialects::ios_like::IosLikeDialect;
    /// use netform_ir::{Node, parse_with_dialect};
    ///
    /// let dialect = IosLikeDialect::new("xr", |_| None)
    ///     .with_block_terminator(|raw| raw.trim() == "end-policy");
    /// let doc = parse_with_dialect("route-policy PASS\n  pass\nend-policy\n", &dialect);
    /// let Node::Block(block) = doc.node(doc.roots[0]).unwrap() else { unreachable!() };
    /// assert_eq!(block.footer.as_ref().unwrap().raw, "end-policy");
    /// ```
    pub const fn with_block_terminator(self, is_terminator: fn(&str) -> bool) -> Self {
        Self {
            name: self.name,
            key_hint: self.key_hint,
            block_terminator: Some(is_terminator),
            empty_block: self.empty_block,
        }
    }

    /// return this dialect with `is_empty_block` answering
    /// [`Dialect::empty_block`].
    ///
    /// ```rust
    /// use netform_dialects::ios_like::IosLikeDialect;
    /// use netform_ir::{Node, parse_with_dialect};
    ///
    /// let dialect = IosLikeDialect::new("xr", |_| None)
    ///     .with_block_terminator(|raw| raw.trim() == "end-set")
    ///     .with_empty_block(|opener, _| opener.starts_with("rd-set "));
    /// let doc = parse_with_dialect("rd-set EMPTY\nend-set\n", &dialect);
    /// let Node::Block(block) = doc.node(doc.roots[0]).unwrap() else { unreachable!() };
    /// assert_eq!(block.footer.as_ref().unwrap().raw, "end-set");
    /// ```
    pub const fn with_empty_block(self, is_empty_block: fn(&str, &str) -> bool) -> Self {
        Self {
            name: self.name,
            key_hint: self.key_hint,
            block_terminator: self.block_terminator,
            empty_block: Some(is_empty_block),
        }
    }
}

impl Dialect for IosLikeDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named(self.name.to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_ios_like_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_parts_with_punctuation(raw, &[])
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
        (self.key_hint)(parsed)
    }

    fn block_terminator(&self, raw: &str) -> bool {
        match self.block_terminator {
            Some(is_terminator) => is_terminator(raw),
            None => false,
        }
    }

    fn empty_block(&self, opener: &str, terminator: &str) -> bool {
        match self.empty_block {
            Some(is_empty_block) => is_empty_block(opener, terminator),
            None => false,
        }
    }

    fn literal_region(&self, raw: &str) -> Option<LiteralTerminator> {
        ios_like_literal_region(raw)
    }
}

/// recognize an IOS-family `banner <type> [delimiter]` line and report what
/// closes the free-text body that follows it.
///
/// - `banner motd ^C` — closed by the next line containing the delimiter.
/// - `banner motd #Warning` — a delimiter glued to the text, closed by the next `#`.
/// - `banner motd` — the delimiter-less EOS form, closed by a line reading `EOF`.
///
/// returns `None` for a banner whose delimiter reappears on the banner line
/// itself, which carries its whole body there.
///
/// # Example
///
/// ```rust
/// use netform_dialects::ios_like::ios_like_literal_region;
/// use netform_ir::LiteralTerminator;
///
/// let region = ios_like_literal_region("banner motd ^C").unwrap();
/// assert!(region.terminates("^C"));
/// assert!(!region.terminates("Authorized use only"));
/// ```
pub fn ios_like_literal_region(raw: &str) -> Option<LiteralTerminator> {
    let after_head = raw.trim_start().strip_prefix("banner")?;
    if !after_head.starts_with([' ', '\t']) {
        return None;
    }

    let (_banner_type, after_type) = split_first_token(after_head)?;
    let Some((delimiter, tail)) = split_first_token(after_type) else {
        return Some(LiteralTerminator::ExactLine("EOF".to_string()));
    };

    if tail.contains(delimiter) {
        return None;
    }

    // a punctuation opener splits glued text off; an alphanumeric token is the
    // delimiter-less word form (`EOF`) and is never split.
    let opener = delimiter_opener(delimiter);
    if delimiter.len() > opener.len() {
        if delimiter.ends_with(opener) {
            return None;
        }
        if !opener.starts_with(char::is_alphanumeric) {
            if delimiter[opener.len()..].contains(opener) || tail.contains(opener) {
                return None;
            }
            return Some(LiteralTerminator::Contains(opener.to_string()));
        }
    }

    Some(LiteralTerminator::Contains(delimiter.to_string()))
}

/// the leading delimiter of a banner token: `^C` anywhere, any other `^X` only
/// as the whole token, otherwise the first character.
fn delimiter_opener(token: &str) -> &str {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return token;
    };
    match chars.next() {
        Some(second)
            if first == '^'
                && (second == 'C' || token.len() == first.len_utf8() + second.len_utf8()) =>
        {
            &token[..first.len_utf8() + second.len_utf8()]
        }
        _ => &token[..first.len_utf8()],
    }
}

/// split leading whitespace and the first whitespace-delimited token off `raw`,
/// returning that token and the untrimmed remainder.
fn split_first_token(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    Some(trimmed.split_at(end))
}

/// the key-hint rules every IOS-family vendor shares, one per head, tried after
/// the vendor's own [`IosRules::key_rules`].
///
/// numbered `access-list N ...` rules have no rule: their identity is their
/// full text, not the ACL number they share.
pub const SHARED_RULES: &[KeyRule] = &[
    shared("interface", interface_key),
    shared("vrf", vrf_key),
    shared("router", router_key),
    shared("ip", ip_key),
    shared("vlan", vlan_key),
    shared("route-map", route_map_key),
    shared("class-map", class_map_key),
    shared("policy-map", policy_map_key),
    shared("ipv6", ipv6_key),
    shared("crypto", crypto_key),
    shared("spanning-tree", spanning_tree_key),
    shared("line", line_key),
    shared("monitor", monitor_key),
    shared("ntp", ntp_key),
];

const fn shared(head: &'static str, derive: fn(&[String], &IosRules) -> Option<String>) -> KeyRule {
    KeyRule {
        head,
        guards: &[],
        action: KeyRuleAction::Derive(derive),
    }
}

fn interface_key(args: &[String], vendor: &IosRules) -> Option<String> {
    let name = args.first()?;
    if let Some((itype, id)) = parse_interface(name, vendor.interface_types) {
        Some(format!("interface:{itype}:{id}"))
    } else {
        Some(format!("interface:{name}"))
    }
}

fn vrf_key(args: &[String], vendor: &IosRules) -> Option<String> {
    match args {
        [sub, name, ..] if sub == vendor.vrf_keyword => Some(format!("vrf:{name}")),
        [name, ..] => Some(format!("vrf:{name}")),
        _ => None,
    }
}

fn router_key(args: &[String], vendor: &IosRules) -> Option<String> {
    match args {
        [proto, id, ..] if proto == "bgp" => Some(format!("router:bgp:{id}")),
        [proto, id, ..] if proto == "ospf" => Some(format!("router:ospf:{id}")),
        [proto, id, ..] if vendor.extra_router_protos.contains(&proto.as_str()) => {
            Some(format!("router:{proto}:{id}"))
        }
        [proto, ..] => Some(format!("router:{proto}")),
        _ => None,
    }
}

fn ip_key(args: &[String], vendor: &IosRules) -> Option<String> {
    match args {
        [next, kind, name, ..] if next == "access-list" => {
            Some(format!("ip-access-list:{kind}:{name}"))
        }
        [next, name] if next == "access-list" => Some(format!("ip-access-list:{name}")),
        [next, rest @ ..] if next == "prefix-list" => prefix_list_key("prefix-list", rest),
        [next, rest @ ..] if next == "route" => {
            static_route_key("ip-route", rest, vendor.route_distance_is_identity)
        }
        _ => None,
    }
}

fn vlan_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        // NX-OS `vlan configuration <id>` is a distinct per-VLAN block;
        // key it on the id so different ids don't collapse to the literal
        // "configuration" (and collide with each other).
        [sub, id, ..] if sub == "configuration" => Some(format!("vlan-configuration:{id}")),
        [id, ..] => Some(format!("vlan:{id}")),
        _ => None,
    }
}

fn route_map_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [name, action, seq, ..] => Some(format!("route-map:{name}:{action}:{seq}")),
        [name, action] => Some(format!("route-map:{name}:{action}")),
        _ => None,
    }
}

fn class_map_key(args: &[String], _: &IosRules) -> Option<String> {
    typed_map_key("class-map", args).or_else(|| match args {
        [_match_kind, name, ..] => Some(format!("class-map:{name}")),
        [name] => Some(format!("class-map:{name}")),
        _ => None,
    })
}

fn policy_map_key(args: &[String], _: &IosRules) -> Option<String> {
    typed_map_key("policy-map", args)
        .or_else(|| args.first().map(|name| format!("policy-map:{name}")))
}

fn ipv6_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [next, name, ..] if next == "access-list" => Some(format!("ipv6-access-list:{name}")),
        [next, rest @ ..] if next == "prefix-list" => prefix_list_key("ipv6-prefix-list", rest),
        [next, rest @ ..] if next == "route" => static_route_key("ipv6-route", rest, false),
        _ => None,
    }
}

fn crypto_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [kind, sub, name, ..] if kind == "ikev2" => Some(format!("crypto:ikev2:{sub}:{name}")),
        [kind, sub, name, param, ..]
            if kind == "ipsec"
                && sub == "security-association"
                && matches!(name.as_str(), "lifetime" | "replay") =>
        {
            Some(format!("crypto:ipsec:{sub}:{name}:{param}"))
        }
        [kind, sub, name, ..] if kind == "ipsec" => Some(format!("crypto:ipsec:{sub}:{name}")),
        [kind, name, seq, ..] if kind == "map" && is_number(seq) => {
            Some(format!("crypto:map:{name}:{seq}"))
        }
        [kind, name] if kind == "map" => Some(format!("crypto:map:{name}")),
        [kind, sub, rest @ ..]
            if kind == "isakmp"
                && matches!(sub.as_str(), "policy" | "profile" | "peer" | "client") =>
        {
            Some(format!("crypto:isakmp:{sub}:{}", rest.join(":")))
        }
        [kind, sub, rest @ ..] if kind == "isakmp" && sub == "key" => {
            let peer = rest
                .iter()
                .rposition(|word| word == "address" || word == "hostname")?;
            Some(format!("crypto:isakmp:key:{}", rest[peer..].join(":")))
        }
        [kind, num, ..] if kind == "isakmp" => Some(format!("crypto:isakmp:{num}")),
        _ => None,
    }
}

fn spanning_tree_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [next, id, param, ..] if next == "vlan" => Some(format!("spanning-tree:vlan:{id}:{param}")),
        [next, id] if next == "vlan" => Some(format!("spanning-tree:vlan:{id}")),
        _ => None,
    }
}

fn line_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [kind, from, to, ..] => Some(format!("line:{kind}:{from}:{to}")),
        [kind, one, ..] => Some(format!("line:{kind}:{one}")),
        _ => None,
    }
}

fn monitor_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [sub, id] if sub == "session" => Some(format!("monitor-session:{id}")),
        [sub, id, kind, ..] if sub == "session" && kind == "type" => {
            Some(format!("monitor-session:{id}"))
        }
        _ => None,
    }
}

fn ntp_key(args: &[String], _: &IosRules) -> Option<String> {
    match args {
        [kind, rest @ ..] if kind == "server" || kind == "peer" => ntp_association_key(kind, rest),
        _ => None,
    }
}

/// key `<head> type <kind...> [match-*] <name>` on its kind and name.
fn typed_map_key(head: &str, args: &[String]) -> Option<String> {
    let [keyword, rest @ .., name] = args else {
        return None;
    };
    let kind = match rest {
        [kind @ .., criterion]
            if matches!(criterion.as_str(), "match-all" | "match-any" | "match-none") =>
        {
            kind
        }
        kind => kind,
    };
    if keyword != "type" || kind.is_empty() {
        return None;
    }
    Some(format!("{head}:{}:{name}", kind.join(":")))
}

/// key a prefix-list on its name, and each of its entries on its sequence number.
fn prefix_list_key(kind: &str, args: &[String]) -> Option<String> {
    match args {
        [name] => Some(format!("{kind}:{name}")),
        [name, keyword, seq, ..] if keyword == "seq" => Some(format!("{kind}:{name}:{seq}")),
        [name, keyword, ..] if keyword == "description" => {
            Some(format!("{kind}:{name}:description"))
        }
        _ => None,
    }
}

/// key an ntp server or peer on its vrf and its address.
fn ntp_association_key(kind: &str, args: &[String]) -> Option<String> {
    let (vrf, args) = match args {
        [keyword, vrf, rest @ ..] if keyword == "vrf" => (Some(vrf), rest),
        _ => (None, args),
    };
    let address = match args {
        [family, address, ..] if family == "ip" || family == "ipv6" => address,
        [address, ..] => address,
        [] => return None,
    };
    Some(match vrf {
        Some(vrf) => format!("ntp:{kind}:{vrf}:{address}"),
        None => format!("ntp:{kind}:{address}"),
    })
}

/// key a static route on its vrf, destination and next hop, and its distance if that is identity.
fn static_route_key(kind: &str, args: &[String], distance_is_identity: bool) -> Option<String> {
    let mut key = kind.to_string();
    let mut words = args;
    if let [keyword, vrf, rest @ ..] = words
        && keyword == "vrf"
    {
        key = format!("{key}:{vrf}");
        words = rest;
    }

    let [destination, rest @ ..] = words else {
        return None;
    };
    let address = destination
        .split_once('/')
        .map_or(destination.as_str(), |(address, _)| address);
    if address.parse::<std::net::IpAddr>().is_err() {
        return None;
    }
    key = format!("{key}:{destination}");
    words = rest;
    if let [mask, rest @ ..] = words
        && !destination.contains('/')
        && let Some(len) = ipv4_mask_length(mask)
    {
        key = format!("{key}/{len}");
        words = rest;
    }

    let end = words
        .iter()
        .position(|word| is_number(word) || ROUTE_ATTRIBUTES.contains(&word.as_str()))
        .unwrap_or(words.len());
    let (next_hop, attributes) = words.split_at(end);
    for word in next_hop {
        key.push(':');
        key.push_str(word);
    }
    if let [distance, ..] = attributes
        && distance_is_identity
        && is_number(distance)
    {
        key.push(':');
        key.push_str(distance);
    }
    Some(key)
}

/// keywords that begin a static route's attributes, as a bare distance does.
const ROUTE_ATTRIBUTES: &[&str] = &["name", "tag", "track", "permanent", "metric"];

/// the prefix length of a contiguous dotted IPv4 mask.
fn ipv4_mask_length(mask: &str) -> Option<u32> {
    let bits = u32::from(mask.parse::<std::net::Ipv4Addr>().ok()?);
    (bits.leading_ones() + bits.trailing_zeros() == 32).then_some(bits.leading_ones())
}

fn is_number(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
}

/// parse an interface name into `(canonical_type, id)` using the given type
/// prefix table.
///
/// uses case-insensitive prefix matching so that any casing of a known
/// interface type normalizes to the canonical lowercase form.  the `types`
/// slice must be ordered longest-prefix-first so that e.g. `tengigabitethernet`
/// matches before `gigabitethernet`.
///
/// returns `None` if the name doesn't match any entry in `types` or has no ID
/// portion after the prefix.
///
/// # Example
///
/// ```rust
/// use netform_dialects::ios_like::parse_interface;
///
/// const TYPES: &[&str] = &["ethernet", "loopback"];
/// assert_eq!(parse_interface("Ethernet1", TYPES), Some(("ethernet", "1")));
/// assert_eq!(parse_interface("Loopback0", TYPES), Some(("loopback", "0")));
/// assert_eq!(parse_interface("Serial0/0", TYPES), None);
/// ```
pub fn parse_interface<'a>(
    name: &'a str,
    types: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    let lower = name.to_ascii_lowercase();
    for &canonical in types {
        if lower.starts_with(canonical) && name.len() > canonical.len() {
            let id = &name[canonical.len()..];
            return Some((canonical, id));
        }
    }
    None
}

/// classify trivia for IOS-like dialects (EOS, IOS XE).
///
/// lines starting with `!` or `#` (after leading whitespace) are comments;
/// blank/whitespace-only lines are blank; everything else is content.
pub fn classify_ios_like_trivia(raw: &str) -> TriviaKind {
    classify_trivia_with_prefixes(raw, &["!", "#"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{Node, parse_with_dialect};

    const PLAIN: IosRules = IosRules {
        interface_types: &["ethernet"],
        vrf_keyword: "context",
        extra_router_protos: &[],
        route_distance_is_identity: false,
        key_rules: &[],
    };

    fn plain_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
        PLAIN.key_hint(parsed)
    }

    fn common_hint(line: &str) -> Option<String> {
        plain_key_hint(parse_parts_with_punctuation(line, &[]).as_ref())
    }

    #[test]
    fn shared_rules_table() {
        for (line, expected) in [
            ("vlan 100", Some("vlan:100")),
            // 0.8.0: `vlan configuration` keys apart from `vlan`.
            ("vlan configuration 10", Some("vlan-configuration:10")),
            (
                "route-map REDISTRIBUTE permit 10",
                Some("route-map:REDISTRIBUTE:permit:10"),
            ),
            ("class-map match-all VOICE", Some("class-map:VOICE")),
            (
                "class-map match-any WEB-TRAFFIC",
                Some("class-map:WEB-TRAFFIC"),
            ),
            ("class-map SIMPLE", Some("class-map:SIMPLE")),
            // 0.10.1: typed class-maps and policy-maps key on type and name.
            (
                "class-map type qos match-all CM-VOICE",
                Some("class-map:qos:CM-VOICE"),
            ),
            (
                "class-map type control-plane match-any copp-s-bgp",
                Some("class-map:control-plane:copp-s-bgp"),
            ),
            (
                "class-map type network-qos c-nq1",
                Some("class-map:network-qos:c-nq1"),
            ),
            (
                "class-map type control subscriber match-none CM-SUB",
                Some("class-map:control:subscriber:CM-SUB"),
            ),
            ("policy-map QOS-POLICY", Some("policy-map:QOS-POLICY")),
            ("policy-map type qos PM-IN", Some("policy-map:qos:PM-IN")),
            (
                "policy-map type control-plane copp-system-p",
                Some("policy-map:control-plane:copp-system-p"),
            ),
            (
                "policy-map type control subscriber PM-SUB",
                Some("policy-map:control:subscriber:PM-SUB"),
            ),
            // see key_hint_numbered_access_list_has_no_hint.
            ("access-list 100 permit ip any any", None),
            (
                "crypto ikev2 proposal PROP-1",
                Some("crypto:ikev2:proposal:PROP-1"),
            ),
            (
                "crypto map VPN-MAP 10 ipsec-isakmp",
                Some("crypto:map:VPN-MAP:10"),
            ),
            ("crypto map VPN-MAP local-address GigabitEthernet0/0", None),
            ("crypto map VPN-MAP", Some("crypto:map:VPN-MAP")),
            ("crypto isakmp policy 10", Some("crypto:isakmp:policy:10")),
            (
                "crypto isakmp profile IKE-PROF",
                Some("crypto:isakmp:profile:IKE-PROF"),
            ),
            (
                "crypto isakmp peer address 192.0.2.1",
                Some("crypto:isakmp:peer:address:192.0.2.1"),
            ),
            (
                "crypto isakmp client configuration group SALES",
                Some("crypto:isakmp:client:configuration:group:SALES"),
            ),
            (
                "crypto isakmp key 6 SECRET address 192.0.2.1",
                Some("crypto:isakmp:key:address:192.0.2.1"),
            ),
            (
                "crypto isakmp key SECRET hostname peer.example.com",
                Some("crypto:isakmp:key:hostname:peer.example.com"),
            ),
            (
                "crypto ipsec security-association lifetime seconds 3600",
                Some("crypto:ipsec:security-association:lifetime:seconds"),
            ),
            (
                "crypto ipsec security-association replay window-size 128",
                Some("crypto:ipsec:security-association:replay:window-size"),
            ),
            (
                "crypto ipsec transform-set AES-SHA esp-aes esp-sha-hmac",
                Some("crypto:ipsec:transform-set:AES-SHA"),
            ),
            (
                "spanning-tree vlan 1-100 priority 4096",
                Some("spanning-tree:vlan:1-100:priority"),
            ),
            ("spanning-tree vlan 10", Some("spanning-tree:vlan:10")),
            ("spanning-tree mode rapid-pvst", None),
            ("line vty 0 4", Some("line:vty:0:4")),
            ("monitor session 1", Some("monitor-session:1")),
            (
                "monitor session 2 type erspan-source",
                Some("monitor-session:2"),
            ),
            ("monitor session 1 source interface Gi1/0/1 both", None),
            ("monitor copp-system-p-policy", None),
            ("ntp server 10.0.0.1", Some("ntp:server:10.0.0.1")),
            ("ntp peer 10.0.0.2", Some("ntp:peer:10.0.0.2")),
            (
                "ntp server vrf MGMT 10.0.0.1",
                Some("ntp:server:MGMT:10.0.0.1"),
            ),
            (
                "ntp server ip ntp1.example.com",
                Some("ntp:server:ntp1.example.com"),
            ),
            ("ntp source-interface mgmt0", None),
            (
                "ipv6 access-list BLOCK-BOGONS",
                Some("ipv6-access-list:BLOCK-BOGONS"),
            ),
            (
                "ipv6 prefix-list DEFAULT-V6-ONLY",
                Some("ipv6-prefix-list:DEFAULT-V6-ONLY"),
            ),
            (
                "ipv6 prefix-list V6 seq 5 permit 2001:db8::/32",
                Some("ipv6-prefix-list:V6:5"),
            ),
            (
                "ipv6 route 2001:db8::/32 Null0",
                Some("ipv6-route:2001:db8::/32:Null0"),
            ),
            (
                "ipv6 route vrf MGMT ::/0 GigabitEthernet0/0 fe80::1",
                Some("ipv6-route:MGMT:::/0:GigabitEthernet0/0:fe80::1"),
            ),
            ("ipv6 unicast-routing", None),
            // the rules that read the vendor's data.
            ("interface Ethernet1", Some("interface:ethernet:1")),
            ("vrf MGMT", Some("vrf:MGMT")),
            ("router bgp 65001", Some("router:bgp:65001")),
            (
                "ip access-list extended ACL",
                Some("ip-access-list:extended:ACL"),
            ),
        ] {
            assert_eq!(common_hint(line), expected.map(String::from), "{line}");
        }
    }

    #[test]
    fn shared_rules_none_on_empty() {
        assert_eq!(plain_key_hint(None), None);
    }

    #[test]
    fn literal_region_table() {
        let contains = |d: &str| Some(LiteralTerminator::Contains(d.into()));
        for (line, expected) in [
            ("banner motd ^C", contains("^C")),
            ("banner login #", contains("#")),
            ("banner motd \u{3}", contains("\u{3}")),
            ("   banner motd ^C", contains("^C")),
            // the delimiter-less EOS form closes on an exact `EOF` line.
            (
                "banner motd",
                Some(LiteralTerminator::ExactLine("EOF".into())),
            ),
            // self-contained banners.
            ("banner motd ^C Hi there ^C", None),
            ("banner motd ^CHi^C", None),
            ("banner motd ^C^C", None),
            ("banner motd #Hi#", None),
            ("banner motd \u{3}Hi\u{3}", None),
            // multi-character delimiters stay whole.
            ("banner motd EOF", contains("EOF")),
            // delimiters glued to the text.
            ("banner motd ^CHi", contains("^C")),
            ("banner motd ^Warning restricted", contains("^")),
            ("banner motd ^1st line", contains("^")),
            // `^Z` + `danger` and `^` + `Zdanger` are indistinguishable (see `delimiter_opener`).
            ("banner motd ^Zdanger", contains("^")),
            ("banner motd #Warning restricted", contains("#")),
            ("banner login @Hi", contains("@")),
            ("banner motd \u{3}Warning", contains("\u{3}")),
            // a one-line banner whose text contains a space.
            ("banner motd #Hello world#", None),
            ("banner motd ^CHello world^C", None),
            ("banner motd ^Hello world^", None),
            ("banner exec %Please log out at %the end%", None),
            ("banner motd \u{3}Hello world\u{3}", None),
            ("banner motd #Hello world", contains("#")),
            ("banner motd ^CHello world", contains("^C")),
            ("banner motd ^Hello world", contains("^")),
            // not banner lines.
            ("interface Ethernet1", None),
            ("bannermotd ^C", None),
            ("no banner motd", None),
            ("banner", None),
            ("banner   ", None),
        ] {
            assert_eq!(ios_like_literal_region(line), expected, "{line:?}");
        }
    }

    #[test]
    fn literal_region_keeps_a_standalone_caret_escape_whole() {
        for delimiter in ["^C", "^Z", "^A", "^X", "^^", "^é"] {
            assert_eq!(
                ios_like_literal_region(&format!("banner motd {delimiter}")),
                Some(LiteralTerminator::Contains(delimiter.into())),
                "{delimiter}",
            );
            assert_eq!(
                ios_like_literal_region(&format!("banner motd {delimiter} Authorized use only")),
                Some(LiteralTerminator::Contains(delimiter.into())),
                "{delimiter}",
            );
        }
    }

    #[test]
    fn ios_like_dialect_reports_no_block_terminator_by_default() {
        let dialect = IosLikeDialect::new("plain", plain_key_hint);

        for raw in ["end", "next", "end-policy", "end-set", "}", "exit"] {
            assert!(!dialect.block_terminator(raw), "{raw} became a terminator");
        }
    }

    #[test]
    fn ios_like_dialect_without_terminator_keeps_closing_lines_as_siblings() {
        let doc = parse_with_dialect(
            "route-policy PASS\n  pass\nend-policy\n",
            &IosLikeDialect::new("plain", plain_key_hint),
        );

        assert_eq!(doc.roots.len(), 2);
        let Some(Node::Block(block)) = doc.node(doc.roots[0]) else {
            panic!("first root should be a block");
        };
        assert!(block.footer.is_none());
    }

    #[test]
    fn ios_like_dialect_with_terminator_attaches_the_closing_line_as_a_footer() {
        let dialect = IosLikeDialect::new("plain", plain_key_hint)
            .with_block_terminator(|raw| raw.trim() == "end-policy");
        let cfg = "route-policy PASS\n  pass\nend-policy\n";
        let doc = parse_with_dialect(cfg, &dialect);

        assert_eq!(doc.roots.len(), 1);
        let Some(Node::Block(block)) = doc.node(doc.roots[0]) else {
            panic!("only root should be a block");
        };
        assert_eq!(
            block.footer.as_ref().map(|f| f.raw.as_str()),
            Some("end-policy")
        );
        assert_eq!(doc.render(), cfg);
    }
}
