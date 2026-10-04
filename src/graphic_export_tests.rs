use super::requested_name_set;

#[test]
fn rejects_duplicate_or_path_like_export_names() {
    assert!(requested_name_set(&["SELECT.DAT".to_owned(), "SELECT.DAT".to_owned()]).is_err());
    assert!(requested_name_set(&["../SELECT.DAT".to_owned()]).is_err());
    assert!(requested_name_set(&["SELECT.DAT".to_owned()]).is_ok());
}
