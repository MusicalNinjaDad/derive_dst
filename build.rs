use build_safely::prelude::*;

fn main() -> Result<()> {
    let mut ac = AutoCfg::new()?;

    let allowed_features = cargo_allowed_features()?;

    // rich compiler diagnostics for the proc-macro itself (via proc_macro2_diagnostic)
    ac.emit_unstable_feature(proc_macro_diagnostic, &allowed_features);
    // needed by crates *using* `#[derive(DST)]` for the field-default syntax
    ac.emit_unstable_feature(default_field_values, &allowed_features);
    // needed by crates *using* `#[derive(DST)]` for the generated `Error::provide`
    ac.emit_unstable_feature(
        OtherFeature("error_generic_member_access".to_string()),
        &allowed_features,
    );
    // needed by crates *using* `#[derive(DST)]` for the generated `ptr::from_raw_parts_mut` call
    ac.emit_unstable_feature(OtherFeature("ptr_metadata".to_string()), &allowed_features);

    Ok(())
}
