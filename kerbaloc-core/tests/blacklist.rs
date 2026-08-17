use kerbaloc_core::blacklist::check;
use std::collections::BTreeMap;

#[test]
fn xscience_keys_are_blacklisted() {
    let mut e = BTreeMap::new();
    e.insert("#LOC_xSci_32".to_string(), "crewReport".to_string());
    let b = check(&e).expect("[x] Science 차단");
    assert_eq!(b.name, "[x] Science!");
    assert!(!b.reason.is_empty());
}

#[test]
fn normal_mods_pass() {
    let mut e = BTreeMap::new();
    e.insert("#LOC_CRP_Karbonite_DisplayName".to_string(), "Karbonite".to_string());
    assert!(check(&e).is_none());
}
