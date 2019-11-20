//! See test_utils/src/marks.rs

test_utils::marks!(
    inserts_angle_brackets_for_generics
    inserts_parens_for_function_calls
    goto_definition_works_for_macros
    goto_definition_works_for_methods
    goto_definition_works_for_fields
    goto_definition_works_for_record_fields
    call_info_bad_offset
    dont_complete_current_use
    dont_complete_primitive_in_use
);
