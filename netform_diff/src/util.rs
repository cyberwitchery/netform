use netform_ir::Path;

pub(crate) fn path_starts_with(path: &[usize], prefix: &[usize]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

pub(crate) fn parent_path(path: &Path) -> Path {
    let mut p = path.0.clone();
    p.pop();
    Path(p)
}

pub(crate) fn key_label(key: Option<u64>) -> String {
    match key {
        Some(v) => format!("0x{v:016x}"),
        None => "<unknown>".to_string(),
    }
}
