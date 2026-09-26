use std::path::PathBuf;

fn rendered() -> String {
    orchbus_api::crds()
        .iter()
        .map(|crd| format!("---\n{}", serde_saphyr::to_string(crd).expect("CRD serializes")))
        .collect()
}

#[test]
fn committed_crds_match_the_types() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/crds/orchbus.yaml");
    let want = rendered();
    if std::env::var_os("UPDATE_CRDS").is_some() {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(have == want, "deploy/crds/orchbus.yaml is stale: run `UPDATE_CRDS=1 cargo test -p orchbus-api`");
}

#[test]
fn every_kind_is_namespaced_versioned_and_in_the_orchbus_category() {
    for crd in orchbus_api::crds() {
        let s = &crd.spec;
        assert_eq!(s.group, orchbus_api::GROUP);
        assert_eq!(s.scope, "Namespaced");
        assert_eq!(s.names.categories.as_deref(), Some(&["orchbus".to_string()][..]));
        assert!(s.names.short_names.as_ref().is_some_and(|n| !n.is_empty()), "{} has no short name", s.names.kind);
        let [v] = &s.versions[..] else { panic!("{} must have one version", s.names.kind) };
        assert!(v.served && v.storage && v.name == orchbus_api::VERSION);
        assert!(v.additional_printer_columns.as_ref().is_some_and(|c| c.iter().any(|c| c.name == "Age")));
    }
}

#[test]
fn no_kind_shadows_a_builtin_short_name() {
    let builtin = ["po", "deploy", "rs", "sts", "cm", "ns", "no", "svc", "role", "sa", "ev", "csr", "ds", "ep", "pv", "pvc"];
    for crd in orchbus_api::crds() {
        for n in crd.spec.names.short_names.unwrap_or_default() {
            assert!(!builtin.contains(&n.as_str()), "{n} shadows a built-in");
        }
        assert_ne!(crd.spec.names.plural, "roles");
    }
}
