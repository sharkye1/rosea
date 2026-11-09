//! Проверяем чтение секции `[Colours]`, цветов комбо и переопределений слайдера.

use std::path::Path;

use rosea_core::error::RoseaError;
use rosea_core::model::{DEFAULT_COMBO_COLOURS, Rgb};
use rosea_core::parse::{Colours, osu_colours};

fn map(colours: &str) -> String {
    format!("osu file format v14\n[Colours]\n{colours}")
}

#[test]
fn reads_custom_combo_colours() {
    let text = "\u{feff}osu file format v14\r\n[General]\r\nMode: 0\r\n\
        [Colours]\r\n\
        Combo1 : 189,134,51\r\n\
        Combo2 : 150,16,16\r\n\
        Combo3 : 222,221,220\r\n\
        Combo4 : 125,92,72\r\n\
        Combo5 : 244,55,55\r\n";

    let colours = osu_colours(text.as_bytes()).unwrap();
    assert_eq!(
        colours,
        Colours {
            combo: vec![
                Rgb::new(189, 134, 51),
                Rgb::new(150, 16, 16),
                Rgb::new(222, 221, 220),
                Rgb::new(125, 92, 72),
                Rgb::new(244, 55, 55),
            ],
            slider_track_override: None,
            slider_border: None,
        }
    );
    assert_eq!(colours.combo[0].to_u32(), 0x00BD8633);
}

#[test]
fn reads_slider_track_and_border_overrides() {
    let text = map("Combo1: 15,237,255\n\
         Combo2: 255,251,85\n\
         SliderTrackOverride: 50, 60, 70\n\
         SliderBorder: 254, 255, 198");

    let colours = osu_colours(text.as_bytes()).unwrap();
    assert_eq!(
        colours.combo,
        vec![Rgb::new(15, 237, 255), Rgb::new(255, 251, 85)]
    );
    assert_eq!(colours.slider_track_override, Some(Rgb::new(50, 60, 70)));
    assert_eq!(colours.slider_border, Some(Rgb::new(254, 255, 198)));
}

#[test]
fn defaults_when_colours_section_is_missing_or_empty() {
    for body in ["", "[Colours]", "[Colours]\n// Combo1: 1,2,3"] {
        let text = format!("osu file format v14\n{body}");
        let colours = osu_colours(text.as_bytes()).unwrap();
        assert_eq!(colours.combo, DEFAULT_COMBO_COLOURS.to_vec());
        assert_eq!(colours.slider_track_override, None);
        assert_eq!(colours.slider_border, None);
    }
}

#[test]
fn orders_combos_by_number_regardless_of_file_line_order() {
    let text = map("Combo3: 0,0,3\nCombo1: 0,0,1\nCombo2: 0,0,2");
    let colours = osu_colours(text.as_bytes()).unwrap();
    assert_eq!(
        colours.combo,
        vec![Rgb::new(0, 0, 1), Rgb::new(0, 0, 2), Rgb::new(0, 0, 3)]
    );
}

#[test]
fn rejects_duplicate_colour_fields() {
    let fields = [
        ("Combo1", "Combo1: 1,1,1\nCombo1: 2,2,2"),
        ("Combo2", "Combo2: 1,1,1\nCombo2: 2,2,2"),
        ("Combo3", "Combo3: 1,1,1\nCombo3: 2,2,2"),
        ("Combo4", "Combo4: 1,1,1\nCombo4: 2,2,2"),
        ("Combo5", "Combo5: 1,1,1\nCombo5: 2,2,2"),
        ("Combo6", "Combo6: 1,1,1\nCombo6: 2,2,2"),
        ("Combo7", "Combo7: 1,1,1\nCombo7: 2,2,2"),
        ("Combo8", "Combo8: 1,1,1\nCombo8: 2,2,2"),
        (
            "SliderTrackOverride",
            "SliderTrackOverride: 1,1,1\nSliderTrackOverride: 2,2,2",
        ),
        ("SliderBorder", "SliderBorder: 1,1,1\nSliderBorder: 2,2,2"),
    ];

    for (name, body) in fields {
        let text = map(body);
        assert_eq!(
            osu_colours(text.as_bytes()),
            Err(RoseaError::DuplicateField),
            "Ожидался DuplicateField для {name}"
        );
    }
}

#[test]
fn rejects_duplicate_colour_fields_across_split_sections() {
    let text = "osu file format v14\n[Colours]\nCombo1: 1,1,1\n\
        [General]\nMode: 0\n[Colours]\nCombo1: 2,2,2";
    assert_eq!(
        osu_colours(text.as_bytes()),
        Err(RoseaError::DuplicateField)
    );
}

#[test]
fn rejects_invalid_rgb_channels() {
    let invalid_values = [
        "256, 0, 0",
        "0, 300, 0",
        "0, 0, 1000",
        "-1, 0, 0",
        "abc, 0, 0",
        "100, def, 0",
        "100, 100, ghi",
        "100, 100",
        "100, 100, 100, 100",
        "100.5, 0, 0",
        "",
        "   ",
    ];

    for value in invalid_values {
        let text = format!("osu file format v14\n[Colours]\nCombo1: {value}");
        assert_eq!(
            osu_colours(text.as_bytes()),
            Err(RoseaError::InvalidColourField {
                field: "Combo1",
                line: 3
            }),
            "Combo1: {value}"
        );

        let text = format!("osu file format v14\n[Colours]\nSliderBorder: {value}");
        assert_eq!(
            osu_colours(text.as_bytes()),
            Err(RoseaError::InvalidColourField {
                field: "SliderBorder",
                line: 3
            }),
            "SliderBorder: {value}"
        );
    }
}

#[test]
fn rejects_supported_keys_without_colon() {
    for key in ["Combo1", "Combo8", "SliderTrackOverride", "SliderBorder"] {
        let text = format!("osu file format v14\n[Colours]\n{key}");
        assert_eq!(
            osu_colours(text.as_bytes()),
            Err(RoseaError::InvalidColourField {
                field: key,
                line: 3
            }),
            "Ключ без двоеточия: {key}"
        );
    }
}

#[test]
fn handles_spaces_comments_and_unknown_fields() {
    let text = "\u{feff}osu file format v14\r\n\
        [General]\r\nMode: 0\r\n\
        [Colours]\r\n\
        // Комментарий\r\n\
        UnknownColourField : 10,20,30\r\n\
        AnotherKeyWithoutColon\r\n\
        Combo1 :   100  ,  150  ,  200  \r\n";

    let colours = osu_colours(text.as_bytes()).unwrap();
    assert_eq!(colours.combo, vec![Rgb::new(100, 150, 200)]);
}

#[test]
fn checks_header_and_mode() {
    assert_eq!(
        osu_colours(b"[Colours]\nCombo1: 1,2,3"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_colours(b"osu file format v13\n[Colours]\nCombo1: 1,2,3"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(
        osu_colours(b"osu file format v14\n[General]\nMode: 1\n[Colours]\nCombo1: 1,2,3"),
        Err(RoseaError::UnsupportedMode(1))
    );
    assert_eq!(osu_colours(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn enforces_line_length_limits() {
    let prefix = "Combo1: 1,2,";
    let mut line = prefix.to_owned();
    line.push_str(&"3".repeat(1024 * 1024 - prefix.len()));
    // Длина строки ровно 1 MiB, но число 333... превышает 255 -> InvalidColourField
    assert_eq!(
        osu_colours(map(&line).as_bytes()),
        Err(RoseaError::InvalidColourField {
            field: "Combo1",
            line: 3
        })
    );

    line.push('3');
    assert_eq!(
        osu_colours(map(&line).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: 1_048_576,
            observed: 1_048_577,
        })
    );
}

#[test]
fn reads_real_fixtures_when_available() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../tests/data");
    if !root.exists() {
        return;
    }

    let dearbrave = root.join("01_micro/dearbrave.osu");
    if dearbrave.exists() {
        let bytes = std::fs::read(&dearbrave).unwrap();
        let colours = osu_colours(&bytes).unwrap();
        assert_eq!(
            colours.combo,
            vec![
                Rgb::new(68, 64, 145),
                Rgb::new(254, 73, 132),
                Rgb::new(151, 198, 255),
                Rgb::new(52, 221, 38),
            ]
        );
        assert_eq!(colours.slider_border, None);
    }

    let pchir = root.join("02_sliders_geometry/1936782_pchir.osu");
    if pchir.exists() {
        let bytes = std::fs::read(&pchir).unwrap();
        let colours = osu_colours(&bytes).unwrap();
        assert_eq!(colours.combo.len(), 4);
        assert_eq!(colours.slider_border, Some(Rgb::new(254, 255, 198)));
    }
}
