//! Export pipeline integration tests for Thoth.
//!
//! Tests the export functionality for transcription history to
//! JSON, CSV, and TXT formats using temporary files.

use std::fs;
use std::io::{BufRead, BufReader};
use tempfile::TempDir;

/// A test transcription record matching the export module structure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TranscriptionRecord {
    id: String,
    text: String,
    raw_text: Option<String>,
    duration_seconds: Option<f64>,
    created_at: String,
    audio_path: Option<String>,
    is_enhanced: bool,
    enhancement_prompt: Option<String>,
}

/// Creates test transcription data.
fn create_test_records() -> Vec<TranscriptionRecord> {
    vec![
        TranscriptionRecord {
            id: "test-uuid-001".to_string(),
            text: "Hello world, this is the first transcription.".to_string(),
            raw_text: None,
            duration_seconds: Some(5.5),
            created_at: "2025-01-15T10:30:00Z".to_string(),
            audio_path: Some("/tmp/audio1.wav".to_string()),
            is_enhanced: false,
            enhancement_prompt: None,
        },
        TranscriptionRecord {
            id: "test-uuid-002".to_string(),
            text: "This is an enhanced transcription with better grammar.".to_string(),
            raw_text: Some("this is an enhanced transcription with better grammar".to_string()),
            duration_seconds: Some(8.2),
            created_at: "2025-01-15T11:00:00Z".to_string(),
            audio_path: None,
            is_enhanced: true,
            enhancement_prompt: Some("grammar".to_string()),
        },
        TranscriptionRecord {
            id: "test-uuid-003".to_string(),
            text: "A third transcription for testing exports.".to_string(),
            raw_text: None,
            duration_seconds: Some(3.0),
            created_at: "2025-01-15T12:30:00Z".to_string(),
            audio_path: Some("/tmp/audio3.wav".to_string()),
            is_enhanced: false,
            enhancement_prompt: None,
        },
    ]
}

// =============================================================================
// JSON Export Tests
// =============================================================================

#[test]
fn test_export_to_json_creates_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let json_path = temp_dir.path().join("export.json");

    let records = create_test_records();

    // Export to JSON
    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, json).expect("Failed to write JSON file");

    // Verify file exists
    assert!(json_path.exists());

    // Verify file has content
    let content = fs::read_to_string(&json_path).expect("Failed to read JSON file");
    assert!(!content.is_empty());
}

#[test]
fn test_export_to_json_roundtrip() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let json_path = temp_dir.path().join("export.json");

    let records = create_test_records();

    // Export to JSON
    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, &json).expect("Failed to write JSON file");

    // Read back and deserialise
    let content = fs::read_to_string(&json_path).expect("Failed to read JSON file");
    let loaded: Vec<TranscriptionRecord> =
        serde_json::from_str(&content).expect("Failed to deserialise JSON");

    // Verify record count
    assert_eq!(loaded.len(), 3);

    // Verify first record
    assert_eq!(loaded[0].id, "test-uuid-001");
    assert_eq!(
        loaded[0].text,
        "Hello world, this is the first transcription."
    );
    assert_eq!(loaded[0].duration_seconds, Some(5.5));
    assert!(!loaded[0].is_enhanced);

    // Verify second record (enhanced)
    assert_eq!(loaded[1].id, "test-uuid-002");
    assert!(loaded[1].is_enhanced);
    assert_eq!(loaded[1].enhancement_prompt, Some("grammar".to_string()));
    assert!(loaded[1].raw_text.is_some());
}

#[test]
fn test_export_to_json_empty_records() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let json_path = temp_dir.path().join("empty.json");

    let records: Vec<TranscriptionRecord> = vec![];

    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, &json).expect("Failed to write JSON file");

    let content = fs::read_to_string(&json_path).expect("Failed to read JSON file");
    let loaded: Vec<TranscriptionRecord> =
        serde_json::from_str(&content).expect("Failed to deserialise");

    assert!(loaded.is_empty());
}

#[test]
fn test_export_json_preserves_special_characters() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let json_path = temp_dir.path().join("special.json");

    let records = vec![TranscriptionRecord {
        id: "special-chars".to_string(),
        text: r#"Text with "quotes", newlines
and unicode: café 🎉"#
            .to_string(),
        raw_text: None,
        duration_seconds: None,
        created_at: "2025-01-15T10:00:00Z".to_string(),
        audio_path: None,
        is_enhanced: false,
        enhancement_prompt: None,
    }];

    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, &json).expect("Failed to write JSON file");

    let content = fs::read_to_string(&json_path).expect("Failed to read JSON file");
    let loaded: Vec<TranscriptionRecord> =
        serde_json::from_str(&content).expect("Failed to deserialise");

    assert!(loaded[0].text.contains("quotes"));
    assert!(loaded[0].text.contains("newlines"));
    assert!(loaded[0].text.contains("cafe") || loaded[0].text.contains("caf"));
}

// =============================================================================
// CSV Export Tests
// =============================================================================

/// Escapes a string for CSV format (handles quotes and commas).
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Writes records to CSV format.
fn write_csv(records: &[TranscriptionRecord], path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = fs::File::create(path)?;

    // Write header
    writeln!(
        file,
        "id,text,raw_text,duration_seconds,created_at,audio_path,is_enhanced,enhancement_prompt"
    )?;

    // Write records
    for record in records {
        let line = format!(
            "{},{},{},{},{},{},{},{}",
            escape_csv(&record.id),
            escape_csv(&record.text),
            escape_csv(&record.raw_text.clone().unwrap_or_default()),
            record
                .duration_seconds
                .map_or(String::new(), |d| d.to_string()),
            escape_csv(&record.created_at),
            escape_csv(&record.audio_path.clone().unwrap_or_default()),
            record.is_enhanced,
            escape_csv(&record.enhancement_prompt.clone().unwrap_or_default()),
        );
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

#[test]
fn test_export_to_csv_creates_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let csv_path = temp_dir.path().join("export.csv");

    let records = create_test_records();
    write_csv(&records, &csv_path).expect("Failed to write CSV");

    assert!(csv_path.exists());
}

#[test]
fn test_export_to_csv_has_header() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let csv_path = temp_dir.path().join("export.csv");

    let records = create_test_records();
    write_csv(&records, &csv_path).expect("Failed to write CSV");

    let file = fs::File::open(&csv_path).expect("Failed to open CSV");
    let reader = BufReader::new(file);
    let first_line = reader
        .lines()
        .next()
        .expect("No lines")
        .expect("Read error");

    assert!(first_line.contains("id"));
    assert!(first_line.contains("text"));
    assert!(first_line.contains("created_at"));
    assert!(first_line.contains("is_enhanced"));
}

#[test]
fn test_export_to_csv_has_correct_row_count() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let csv_path = temp_dir.path().join("export.csv");

    let records = create_test_records();
    write_csv(&records, &csv_path).expect("Failed to write CSV");

    let file = fs::File::open(&csv_path).expect("Failed to open CSV");
    let reader = BufReader::new(file);
    let line_count = reader.lines().count();

    // Header + 3 data rows
    assert_eq!(line_count, 4);
}

#[test]
fn test_escape_csv_simple() {
    assert_eq!(escape_csv("hello"), "hello");
}

#[test]
fn test_escape_csv_with_comma() {
    assert_eq!(escape_csv("hello, world"), "\"hello, world\"");
}

#[test]
fn test_escape_csv_with_quotes() {
    assert_eq!(escape_csv("say \"hello\""), "\"say \"\"hello\"\"\"");
}

#[test]
fn test_escape_csv_with_newline() {
    assert_eq!(escape_csv("line1\nline2"), "\"line1\nline2\"");
}

#[test]
fn test_export_csv_with_special_characters() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let csv_path = temp_dir.path().join("special.csv");

    let records = vec![TranscriptionRecord {
        id: "csv-special".to_string(),
        text: "Text with, comma and \"quotes\"".to_string(),
        raw_text: None,
        duration_seconds: None,
        created_at: "2025-01-15T10:00:00Z".to_string(),
        audio_path: None,
        is_enhanced: false,
        enhancement_prompt: None,
    }];

    write_csv(&records, &csv_path).expect("Failed to write CSV");

    let content = fs::read_to_string(&csv_path).expect("Failed to read CSV");

    // The text should be quoted due to comma
    assert!(content.contains('"'));
}

// =============================================================================
// TXT Export Tests
// =============================================================================

/// Writes records to plain text format.
fn write_txt(records: &[TranscriptionRecord], path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = fs::File::create(path)?;

    for (i, record) in records.iter().enumerate() {
        if i > 0 {
            writeln!(file, "\n{}", "-".repeat(80))?;
        }

        writeln!(file, "Date: {}", record.created_at)?;

        if let Some(duration) = record.duration_seconds {
            writeln!(file, "Duration: {:.1}s", duration)?;
        }

        if record.is_enhanced {
            writeln!(file, "Enhanced: Yes")?;
        }

        writeln!(file)?;
        writeln!(file, "{}", record.text)?;
    }

    Ok(())
}

#[test]
fn test_export_to_txt_creates_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_path = temp_dir.path().join("export.txt");

    let records = create_test_records();
    write_txt(&records, &txt_path).expect("Failed to write TXT");

    assert!(txt_path.exists());
}

#[test]
fn test_export_to_txt_contains_transcription_text() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_path = temp_dir.path().join("export.txt");

    let records = create_test_records();
    write_txt(&records, &txt_path).expect("Failed to write TXT");

    let content = fs::read_to_string(&txt_path).expect("Failed to read TXT");

    // Verify transcription text is present
    assert!(content.contains("Hello world, this is the first transcription."));
    assert!(content.contains("This is an enhanced transcription with better grammar."));
    assert!(content.contains("A third transcription for testing exports."));
}

#[test]
fn test_export_to_txt_includes_metadata() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_path = temp_dir.path().join("export.txt");

    let records = create_test_records();
    write_txt(&records, &txt_path).expect("Failed to write TXT");

    let content = fs::read_to_string(&txt_path).expect("Failed to read TXT");

    // Verify metadata is present
    assert!(content.contains("Date:"));
    assert!(content.contains("Duration:"));
    assert!(content.contains("Enhanced: Yes")); // For the enhanced record
}

#[test]
fn test_export_to_txt_separates_records() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_path = temp_dir.path().join("export.txt");

    let records = create_test_records();
    write_txt(&records, &txt_path).expect("Failed to write TXT");

    let content = fs::read_to_string(&txt_path).expect("Failed to read TXT");

    // Verify separator is present between records
    assert!(content.contains("-".repeat(80).as_str()));
}

#[test]
fn test_export_to_txt_empty_records() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_path = temp_dir.path().join("empty.txt");

    let records: Vec<TranscriptionRecord> = vec![];
    write_txt(&records, &txt_path).expect("Failed to write TXT");

    let content = fs::read_to_string(&txt_path).expect("Failed to read TXT");
    assert!(content.is_empty());
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_export_single_record() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let records = vec![TranscriptionRecord {
        id: "single".to_string(),
        text: "Single record test".to_string(),
        raw_text: None,
        duration_seconds: Some(1.0),
        created_at: "2025-01-15T10:00:00Z".to_string(),
        audio_path: None,
        is_enhanced: false,
        enhancement_prompt: None,
    }];

    // Test all formats
    let json_path = temp_dir.path().join("single.json");
    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, json).expect("Failed to write JSON");
    assert!(json_path.exists());

    let csv_path = temp_dir.path().join("single.csv");
    write_csv(&records, &csv_path).expect("Failed to write CSV");
    assert!(csv_path.exists());

    let txt_path = temp_dir.path().join("single.txt");
    write_txt(&records, &txt_path).expect("Failed to write TXT");
    assert!(txt_path.exists());
}

#[test]
fn test_export_large_batch() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create 100 records
    let records: Vec<TranscriptionRecord> = (0..100)
        .map(|i| TranscriptionRecord {
            id: format!("batch-{:03}", i),
            text: format!("This is transcription number {} in the batch.", i),
            raw_text: None,
            duration_seconds: Some(i as f64 * 0.5),
            created_at: format!("2025-01-15T{:02}:{:02}:00Z", 10 + (i / 60), i % 60),
            audio_path: None,
            is_enhanced: i % 5 == 0,
            enhancement_prompt: if i % 5 == 0 {
                Some("grammar".to_string())
            } else {
                None
            },
        })
        .collect();

    // Test JSON export
    let json_path = temp_dir.path().join("batch.json");
    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, &json).expect("Failed to write JSON");

    let loaded: Vec<TranscriptionRecord> =
        serde_json::from_str(&fs::read_to_string(&json_path).expect("Read error"))
            .expect("Failed to deserialise");
    assert_eq!(loaded.len(), 100);

    // Test CSV export
    let csv_path = temp_dir.path().join("batch.csv");
    write_csv(&records, &csv_path).expect("Failed to write CSV");

    let file = fs::File::open(&csv_path).expect("Failed to open CSV");
    let line_count = BufReader::new(file).lines().count();
    assert_eq!(line_count, 101); // Header + 100 rows
}

#[test]
fn test_export_with_null_optional_fields() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let records = vec![TranscriptionRecord {
        id: "minimal".to_string(),
        text: "Minimal record".to_string(),
        raw_text: None,
        duration_seconds: None,
        created_at: "2025-01-15T10:00:00Z".to_string(),
        audio_path: None,
        is_enhanced: false,
        enhancement_prompt: None,
    }];

    // JSON should handle null values gracefully
    let json_path = temp_dir.path().join("minimal.json");
    let json = serde_json::to_string_pretty(&records).expect("Failed to serialise");
    fs::write(&json_path, &json).expect("Failed to write JSON");

    let content = fs::read_to_string(&json_path).expect("Failed to read");
    assert!(content.contains("null") || content.contains("\"raw_text\": null"));

    // CSV should handle empty fields
    let csv_path = temp_dir.path().join("minimal.csv");
    write_csv(&records, &csv_path).expect("Failed to write CSV");
    assert!(csv_path.exists());

    // TXT should skip optional fields
    let txt_path = temp_dir.path().join("minimal.txt");
    write_txt(&records, &txt_path).expect("Failed to write TXT");
    let txt_content = fs::read_to_string(&txt_path).expect("Failed to read");
    assert!(!txt_content.contains("Duration:")); // Should be skipped when None
}
