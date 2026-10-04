use std::path::PathBuf;

use super::{
    TranslationTerminology, WORDING_STATES, audit_translation_assets,
    validate_translation_terminology,
};

#[test]
#[ignore = "requires the translation assets in assets/translations"]
fn audits_the_tracked_scope_workflow_and_terminology() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = audit_translation_assets(
        &root.join("assets/translations/policy.json"),
        &root.join("assets/translations/terminology.json"),
    )
    .unwrap();

    assert_eq!(report.wording_status_count, WORDING_STATES.len());
    assert!(report.terminology_entry_count >= 19);
    assert_eq!(report.reference_project_count, 2);

    let bytes = std::fs::read(root.join("assets/translations/terminology.json")).unwrap();
    let terminology: TranslationTerminology = serde_json::from_slice(&bytes).unwrap();
    for required_source_term in [
        "カーバンクル",
        "アイスストーム",
        "アレイアード",
        "すけとうだら",
        "うろこさかなびと",
    ] {
        assert!(
            terminology
                .entries
                .iter()
                .any(|entry| entry.source_term == required_source_term)
        );
    }
}

#[test]
#[ignore = "requires the translation assets in assets/translations"]
fn rejects_a_duplicate_term_even_when_the_korean_spelling_matches() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(root.join("assets/translations/terminology.json")).unwrap();
    let mut terminology: TranslationTerminology = serde_json::from_slice(&bytes).unwrap();
    terminology.entries[1].source_term = terminology.entries[0].source_term.clone();

    assert!(validate_translation_terminology(&terminology).is_err());
}

#[test]
#[ignore = "requires the translation assets in assets/translations"]
fn rejects_terms_that_do_not_occur_in_the_target_catalog() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(root.join("assets/translations/terminology.json")).unwrap();
    let mut terminology: TranslationTerminology = serde_json::from_slice(&bytes).unwrap();
    terminology.entries[0].target_occurrence_count = 0;

    assert!(validate_translation_terminology(&terminology).is_err());
}

#[test]
#[ignore = "requires the translation assets in assets/translations"]
fn dynamic_item_sentences_keep_the_runtime_particle_required_by_their_korean_grammar() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/translations/batches");
    for (file, id, segment_index, marker) in [
        (
            "common-acquisition-and-configuration-ko.json",
            "MSG.DAT:82",
            0,
            "{josa:을}",
        ),
        (
            "common-acquisition-and-configuration-ko.json",
            "MSG.DAT:83",
            0,
            "{josa:을}",
        ),
        ("voice-arle-ko.json", "MSG01.DAT:7C", 1, "{josa:이}"),
        ("voice-satan-ko.json", "MSG02.DAT:7C", 1, "{josa:이}"),
        ("voice-schezo-ko.json", "MSG03.DAT:82", 2, "{josa:은}"),
        ("voice-incubus-ko.json", "MSG04.DAT:7C", 1, "{josa:이}"),
        ("voice-incubus-ko.json", "MSG04.DAT:84", 1, "{josa:을}"),
        ("voice-rulue-ko.json", "MSG05.DAT:7C", 1, "{josa:이}"),
        ("voice-rulue-ko.json", "MSG05.DAT:82", 2, "{josa:은}"),
        ("voice-minotauros-ko.json", "MSG06.DAT:84", 1, "{josa:을}"),
        ("voice-draco-ko.json", "MSG07.DAT:7C", 1, "{josa:이}"),
        ("voice-draco-ko.json", "MSG07.DAT:82", 1, "{josa:은}"),
        ("voice-witch-ko.json", "MSG08.DAT:7C", 1, "{josa:이}"),
        ("voice-witch-ko.json", "MSG08.DAT:82", 2, "{josa:은}"),
        (
            "voice-elephant-daimo-ko.json",
            "MSG09.DAT:7C",
            1,
            "{josa:이}",
        ),
        (
            "voice-elephant-daimo-ko.json",
            "MSG09.DAT:84",
            1,
            "{josa:을}",
        ),
        ("voice-sasoriman-ko.json", "MSG10.DAT:7C", 1, "{josa:이}"),
        ("voice-sasoriman-ko.json", "MSG10.DAT:84", 1, "{josa:을}"),
        ("voice-suketoudara-ko.json", "MSG11.DAT:7C", 1, "{josa:이}"),
        ("voice-suketoudara-ko.json", "MSG11.DAT:84", 1, "{josa:을}"),
        ("voice-fishwoman-ko.json", "MSG12.DAT:7C", 1, "{josa:이}"),
    ] {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(file)).unwrap()).unwrap();
        let entry = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap();
        assert!(
            entry["ko_segments"][segment_index]
                .as_str()
                .unwrap()
                .starts_with(marker),
            "{file} {id} must start segment {segment_index} with {marker}"
        );
    }

    let items: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("common-item-names-ko.json")).unwrap())
            .unwrap();
    let endings = items["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["ko_segments"][0].as_str()?.chars().last())
        .map(|character| crate::josa::has_batchim(character).unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(endings, std::collections::BTreeSet::from([false, true]));
}

#[test]
#[ignore = "requires the translation assets in assets/translations"]
fn review_batches_keep_each_character_voice_and_separable_ui_role_together() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/translations");
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("catalog.json")).unwrap()).unwrap();
    let contexts: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("contexts.json")).unwrap()).unwrap();

    let isolated_contexts = contexts["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|context| {
            let id = context["id"].as_str().unwrap();
            let independently_reviewed = context["surface"] == "character_voice"
                || matches!(
                    id,
                    "common_team_and_character_names"
                        | "common_turn_and_weather_status"
                        | "common_menu_status_and_store_ui"
                        | "staff_title_and_role_labels"
                        | "staff_personal_and_company_names"
                        | "arle_satan_unpredictable_spell_outcomes"
                );
            independently_reviewed.then_some(id)
        })
        .collect::<std::collections::BTreeSet<_>>();

    let mut context_owners = std::collections::BTreeMap::<String, String>::new();
    let mut selected_ids = std::collections::BTreeSet::new();
    for batch in catalog["batches"].as_array().unwrap() {
        let batch_id = batch["id"].as_str().unwrap();
        assert_ne!(batch_id, "representative_sample");
        let selection: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(batch["selection"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let batch_contexts = selection["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["context_id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        for context_id in &batch_contexts {
            assert!(
                context_owners
                    .insert((*context_id).to_owned(), batch_id.to_owned())
                    .is_none(),
                "review context {context_id} is split across batches"
            );
        }
        for entry in selection["entries"].as_array().unwrap() {
            assert!(selected_ids.insert(entry["id"].as_str().unwrap().to_owned()));
        }
        if batch_contexts
            .iter()
            .any(|id| isolated_contexts.contains(id))
        {
            assert_eq!(batch_contexts.len(), 1);
            assert_eq!(batch_id, *batch_contexts.first().unwrap());
        }
    }

    assert_eq!(selected_ids.len(), 1_621);
    for context_id in isolated_contexts {
        assert_eq!(
            context_owners.get(context_id).map(String::as_str),
            Some(context_id)
        );
    }

    let representative: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("representative-sample.json")).unwrap())
            .unwrap();
    assert!(
        representative["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| { selected_ids.contains(entry["id"].as_str().unwrap()) })
    );
}
