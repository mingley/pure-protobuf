//! Consumers of two generated targets, including an imported required message.
include!("mod.rs");

#[cfg(test)]
mod tests {
    use super::{
        required_import::ImportedParent,
        required_merge::{Parent, Wide},
    };
    use pbrs::Parse;

    #[test]
    fn imported_required_message_merges_partial_occurrences() {
        let wire = [10, 2, 8, 1, 10, 2, 16, 2];
        let imported = ImportedParent::parse(&wire).unwrap();
        let same_file = Parent::parse(&wire).unwrap();
        assert_eq!((imported.child().a(), imported.child().b()), (1, 2));
        assert_eq!((same_file.child().a(), same_file.child().b()), (1, 2));
    }

    #[test]
    fn imported_required_message_rejects_a_final_missing_field() {
        let wire = [10, 2, 8, 1];
        assert!(ImportedParent::parse(&wire).is_err());
        assert!(Parent::parse(&wire).is_err());
        let mut pos = 0;
        assert!(
            ImportedParent::validate_inner(&pbrs::rt::Wire::from_slice(&wire), &mut pos, 0)
                .is_err()
        );
    }

    #[test]
    fn wide_required_message_also_works_in_parallel_output() {
        let mut wire = Vec::new();
        for number in 1..=65 {
            pbrs::rt::encode_tag(&mut wire, number, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(&mut wire, 0);
        }
        assert!(Wide::parse(&wire).is_ok());
    }
}
