pub(crate) const TREE_TRAVERSAL_DEPTH_LIMIT: u32 = 1024;

pub(crate) fn should_visit_tree_depth(depth: u32) -> bool {
    depth <= TREE_TRAVERSAL_DEPTH_LIMIT
}

pub(crate) fn child_tree_depth(depth: u32) -> Option<u32> {
    if depth >= TREE_TRAVERSAL_DEPTH_LIMIT {
        return None;
    }

    depth.checked_add(1)
}
