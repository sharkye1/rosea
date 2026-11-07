//! Проверяем чтение секции [Metadata], поддержку Юникода и кириллицы,
//! валидацию повторов полей и граничные случаи.

use std::path::Path;

use rosea_core::error::RoseaError;
use rosea_core::parse::{Metadata, osu_metadata};

fn map(meta: &str) -> String {
    format!("osu file format v14\n[Metadata]\n{meta}")
}

#[test]
fn reads_all_metadata_fields() {
    let text = "\u{feff}osu file format v14\r\n[General]\r\nMode: 0\r\n\
        [Metadata]\r\n\
        Title:Song Title\r\n\
        TitleUnicode:Song Title (Unicode)\r\n\
        Artist:Song Artist\r\n\
        ArtistUnicode:Song Artist (Unicode)\r\n\
        Creator:Mapper Name\r\n\
        Version:Insane\r\n\
        Source:Original Anime\r\n\
        Tags:tag1 tag2 tag3\r\n\
        BeatmapID:123456\r\n\
        BeatmapSetID:654321\r\n\
        [Difficulty]\r\nCircleSize: 4\r\n";

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(
        meta,
        Metadata {
            title: "Song Title".to_owned(),
            title_unicode: Some("Song Title (Unicode)".to_owned()),
            artist: "Song Artist".to_owned(),
            artist_unicode: Some("Song Artist (Unicode)".to_owned()),
            creator: "Mapper Name".to_owned(),
            version: "Insane".to_owned(),
            source: "Original Anime".to_owned(),
            tags: "tag1 tag2 tag3".to_owned(),
            beatmap_id: Some(123456),
            beatmap_set_id: Some(654321),
        }
    );
    assert_eq!(meta.title_unicode_or_title(), "Song Title (Unicode)");
    assert_eq!(meta.artist_unicode_or_artist(), "Song Artist (Unicode)");
}

#[test]
fn reads_cyrillic_metadata() {
    let text = map("Title:Gruppa Krovi\n\
         TitleUnicode:Группа крови\n\
         Artist:Kino\n\
         ArtistUnicode:Кино\n\
         Creator:Виктор Цой\n\
         Version:Сложно (Хард)\n\
         Source:Фильм «Игла»\n\
         Tags:русский рок пост-панк цой кино 1988\n\
         BeatmapID:777\n\
         BeatmapSetID:888");

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "Gruppa Krovi");
    assert_eq!(meta.title_unicode.as_deref(), Some("Группа крови"));
    assert_eq!(meta.artist, "Kino");
    assert_eq!(meta.artist_unicode.as_deref(), Some("Кино"));
    assert_eq!(meta.creator, "Виктор Цой");
    assert_eq!(meta.version, "Сложно (Хард)");
    assert_eq!(meta.source, "Фильм «Игла»");
    assert_eq!(meta.tags, "русский рок пост-панк цой кино 1988");
    assert_eq!(meta.beatmap_id, Some(777));
    assert_eq!(meta.beatmap_set_id, Some(888));

    // Проверяем работу методов выбора отображаемого имени
    assert_eq!(meta.title_unicode_or_title(), "Группа крови");
    assert_eq!(meta.artist_unicode_or_artist(), "Кино");
}

#[test]
fn reads_pure_cyrillic_without_unicode_fields() {
    let text = map("Title:Звезда по имени Солнце\n\
         Artist:Кино\n\
         Creator:Рокер\n\
         Version:Обычная");

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "Звезда по имени Солнце");
    assert_eq!(meta.title_unicode, None);
    assert_eq!(meta.artist, "Кино");
    assert_eq!(meta.artist_unicode, None);

    // При отсутствии отдельных Unicode-полей возвращается романизированный вариант (где и записан Юникод)
    assert_eq!(meta.title_unicode_or_title(), "Звезда по имени Солнце");
    assert_eq!(meta.artist_unicode_or_artist(), "Кино");
}

#[test]
fn reads_japanese_and_cjk_metadata() {
    let text = map("Title:Dear Brave\n\
         TitleUnicode:ディアブレイブ\n\
         Artist:Kano\n\
         ArtistUnicode:鹿乃\n\
         Creator:Fycho\n\
         Version:test\n\
         Source:ヘヴィーオブジェクト\n\
         Tags:Heavy Object ending full ver thzz cah-m\n\
         BeatmapID:0\n\
         BeatmapSetID:-1");

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "Dear Brave");
    assert_eq!(meta.title_unicode.as_deref(), Some("ディアブレイブ"));
    assert_eq!(meta.artist, "Kano");
    assert_eq!(meta.artist_unicode.as_deref(), Some("鹿乃"));
    assert_eq!(meta.source, "ヘヴィーオブジェクト");
    assert_eq!(meta.title_unicode_or_title(), "ディアブレイブ");
    assert_eq!(meta.artist_unicode_or_artist(), "鹿乃");
}

#[test]
fn handles_empty_and_default_metadata() {
    for body in ["", "[Metadata]", "[Metadata]\nSource:\nTags:"] {
        let text = format!("osu file format v14\n{body}");
        let meta = osu_metadata(text.as_bytes()).unwrap();
        assert_eq!(meta.title, "");
        assert_eq!(meta.title_unicode, None);
        assert_eq!(meta.artist, "");
        assert_eq!(meta.artist_unicode, None);
        assert_eq!(meta.creator, "");
        assert_eq!(meta.version, "");
        assert_eq!(meta.source, "");
        assert_eq!(meta.tags, "");
        assert_eq!(meta.beatmap_id, None);
        assert_eq!(meta.beatmap_set_id, None);
        assert_eq!(meta.title_unicode_or_title(), "");
        assert_eq!(meta.artist_unicode_or_artist(), "");
    }
}

#[test]
fn falls_back_to_title_and_artist_when_unicode_is_empty() {
    let text = map("Title:My Title\n\
         TitleUnicode:\n\
         Artist:My Artist\n\
         ArtistUnicode:  ");

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "My Title");
    assert_eq!(meta.title_unicode.as_deref(), Some(""));
    assert_eq!(meta.artist, "My Artist");
    assert_eq!(meta.artist_unicode.as_deref(), Some(""));

    // Пустые Unicode-поля не должны использоваться при отображении
    assert_eq!(meta.title_unicode_or_title(), "My Title");
    assert_eq!(meta.artist_unicode_or_artist(), "My Artist");
}

#[test]
fn rejects_duplicate_metadata_fields() {
    let fields = [
        ("Title", "Title: A\nTitle: B"),
        ("TitleUnicode", "TitleUnicode: A\nTitleUnicode: B"),
        ("Artist", "Artist: A\nArtist: B"),
        ("ArtistUnicode", "ArtistUnicode: A\nArtistUnicode: B"),
        ("Creator", "Creator: A\nCreator: B"),
        ("Version", "Version: A\nVersion: B"),
        ("Source", "Source: A\nSource: B"),
        ("Tags", "Tags: A\nTags: B"),
        ("BeatmapID", "BeatmapID: 1\nBeatmapID: 2"),
        ("BeatmapSetID", "BeatmapSetID: 1\nBeatmapSetID: 2"),
    ];

    for (name, body) in fields {
        let text = map(body);
        assert_eq!(
            osu_metadata(text.as_bytes()),
            Err(RoseaError::DuplicateField),
            "Ожидался DuplicateField для поля {name}"
        );
    }
}

#[test]
fn rejects_duplicate_metadata_fields_across_split_sections() {
    let fields = [
        "Title: A",
        "TitleUnicode: A",
        "Artist: A",
        "ArtistUnicode: A",
        "Creator: A",
        "Version: A",
        "Source: A",
        "Tags: A",
        "BeatmapID: 1",
        "BeatmapSetID: 1",
    ];

    for field in fields {
        let text = format!(
            "osu file format v14\n[Metadata]\n{field}\n[General]\nMode: 0\n[Metadata]\n{field}"
        );
        assert_eq!(
            osu_metadata(text.as_bytes()),
            Err(RoseaError::DuplicateField),
            "Ожидался DuplicateField для раздельных секций с {field}"
        );
    }
}

#[test]
fn accepts_negative_and_zero_beatmap_ids() {
    let text = map("BeatmapID: 0\nBeatmapSetID: -1");
    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.beatmap_id, Some(0));
    assert_eq!(meta.beatmap_set_id, Some(-1));

    let text = map("BeatmapID: -100\nBeatmapSetID: -999");
    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.beatmap_id, Some(-100));
    assert_eq!(meta.beatmap_set_id, Some(-999));
}

#[test]
fn rejects_invalid_numeric_metadata_fields() {
    for value in ["abc", "12.34", "NaN", "", "   ", "100a", "- 5"] {
        let text = format!("osu file format v14\n[Metadata]\n\nBeatmapID: {value}");
        assert_eq!(
            osu_metadata(text.as_bytes()),
            Err(RoseaError::InvalidMetadataField {
                field: "BeatmapID",
                line: 4,
            }),
            "BeatmapID: {value}"
        );

        let text = format!("osu file format v14\n[Metadata]\nBeatmapSetID: {value}");
        assert_eq!(
            osu_metadata(text.as_bytes()),
            Err(RoseaError::InvalidMetadataField {
                field: "BeatmapSetID",
                line: 3,
            }),
            "BeatmapSetID: {value}"
        );
    }
}

#[test]
fn rejects_supported_keys_without_colon() {
    let keys = [
        "Title",
        "TitleUnicode",
        "Artist",
        "ArtistUnicode",
        "Creator",
        "Version",
        "Source",
        "Tags",
        "BeatmapID",
        "BeatmapSetID",
    ];

    for key in keys {
        let text = format!("osu file format v14\n[Metadata]\n{key}");
        assert_eq!(
            osu_metadata(text.as_bytes()),
            Err(RoseaError::InvalidMetadataField {
                field: key,
                line: 3
            }),
            "Ключ без двоеточия: {key}"
        );
    }
}

#[test]
fn handles_colons_and_whitespace_in_values() {
    let text = map(" Title : Re:Zero - Starting Life in Another World \n\
         Source: Anime: Episode 1: The Beginning \n\
         Artist :  Artist : With : Colons  ");

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "Re:Zero - Starting Life in Another World");
    assert_eq!(meta.source, "Anime: Episode 1: The Beginning");
    assert_eq!(meta.artist, "Artist : With : Colons");
}

#[test]
fn ignores_comments_unknown_fields_and_other_sections() {
    let text = "\u{feff}osu file format v14\r\n\
        [General]\r\n\
        Title: ShouldBeIgnored\r\n\
        Mode: 0\r\n\
        [Metadata]\r\n\
        // Title: CommentTitle\r\n\
        UnknownCustomField: CustomValue\r\n\
        AnotherKeyWithoutColon\r\n\
        Title: Real Title\r\n\
        [Difficulty]\r\n\
        Title: DifficultyTitle\r\n";

    let meta = osu_metadata(text.as_bytes()).unwrap();
    assert_eq!(meta.title, "Real Title");
}

#[test]
fn checks_header_and_mode() {
    assert_eq!(
        osu_metadata(b"[Metadata]\nTitle: Test"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_metadata(b"osu file format v13\n[Metadata]\nTitle: Test"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(
        osu_metadata(b"osu file format v14\n[General]\nMode: 1\n[Metadata]\nTitle: Test"),
        Err(RoseaError::UnsupportedMode(1))
    );
    assert_eq!(osu_metadata(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn enforces_line_length_limits() {
    let prefix = "Title: ";
    let mut line = prefix.to_owned();
    line.push_str(&"a".repeat(1024 * 1024 - prefix.len()));
    let meta = osu_metadata(map(&line).as_bytes()).unwrap();
    assert_eq!(meta.title.len(), 1024 * 1024 - prefix.len());

    line.push('a');
    assert_eq!(
        osu_metadata(map(&line).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: 1_048_576,
            observed: 1_048_577,
        })
    );
}

#[test]
fn reads_real_fixtures_when_available() {
    let dearbrave = Path::new("tests/data/01_micro/dearbrave.osu");
    if dearbrave.exists() {
        let bytes = std::fs::read(dearbrave).unwrap();
        let meta = osu_metadata(&bytes).unwrap();
        assert_eq!(meta.title, "Dear Brave");
        assert_eq!(meta.title_unicode.as_deref(), Some("ディアブレイブ"));
        assert_eq!(meta.artist, "Kano");
        assert_eq!(meta.artist_unicode.as_deref(), Some("鹿乃"));
        assert_eq!(meta.creator, "Fycho");
        assert_eq!(meta.version, "test");
        assert_eq!(meta.source, "ヘヴィーオブジェクト");
        assert_eq!(meta.tags, "Heavy Object ending full ver thzz cah-m");
        assert_eq!(meta.beatmap_id, Some(0));
        assert_eq!(meta.beatmap_set_id, Some(-1));
    }

    let yomi = Path::new("tests/data/01_micro/yomi.osu");
    if yomi.exists() {
        let bytes = std::fs::read(yomi).unwrap();
        let meta = osu_metadata(&bytes).unwrap();
        assert_eq!(
            meta.title,
            "Yomi yori Kikoyu, Koukoku no Tou to Honoo no Shoujo."
        );
        assert_eq!(
            meta.title_unicode.as_deref(),
            Some("黄泉より聴こゆ、皇国の燈と焔の少女。")
        );
        assert_eq!(meta.artist, "Imperial Circus Dead Decadence");
        assert_eq!(meta.creator, "DoKito");
        assert_eq!(meta.beatmap_set_id, Some(461744));
    }
}
