//! The largest file a command reads, as `commands` limits it.

use crate::commands::*;

#[test]
fn an_ifc_file_may_be_twice_as_large_as_a_project_so_issue_264s_opens() {
    // Issue #264: an IFC file of 282,607,463 bytes was refused as too large; a project or a family keeps its 256 MiB.
    assert!(max_file("ifc") > 282_607_463);
    assert_eq!(max_file("ifc"), 512 << 20);
    for other in ["bimcraft", "json", "", "ifczip", "IFC"] {
        assert_eq!(max_file(other), MAX_FILE, "{other}");
    }
}
