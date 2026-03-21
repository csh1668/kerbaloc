"""Tests for the KSP cfg parser."""

from pathlib import Path

from ksp_translator.parser import parse_loc_file

FIXTURE = Path(__file__).parent / "fixtures" / "sample_en-us.cfg"


def test_parse_extracts_loc_keys():
    entries = parse_loc_file(FIXTURE)
    assert "#LOC_MyMod_Title" in entries
    assert entries["#LOC_MyMod_Title"] == "Super Engine"


def test_parse_extracts_autoloc():
    entries = parse_loc_file(FIXTURE)
    assert "#autoLOC_12345" in entries
    assert entries["#autoLOC_12345"] == "This is an automatic localization entry for testing."


def test_parse_preserves_format_tags():
    entries = parse_loc_file(FIXTURE)
    assert "<<1>>" in entries["#LOC_MyMod_Desc"]


def test_parse_ignores_comments():
    entries = parse_loc_file(FIXTURE)
    for key in entries:
        assert not key.startswith("//")


def test_parse_extracts_all_expected_keys():
    entries = parse_loc_file(FIXTURE)
    expected = {
        "#LOC_MyMod_Title",
        "#LOC_MyMod_Desc",
        "#LOC_MyMod_Short",
        "#LOC_MyMod_Hybrid^N",
        "#LOC_MyMod_Format",
        "#LOC_MyMod_Unit",
        "#autoLOC_12345",
        "#LOC_MyMod_Multiword",
    }
    assert set(entries.keys()) == expected


def test_parse_bom_file(tmp_path):
    bom_file = tmp_path / "bom.cfg"
    content = "\ufeffLocalization\n{\n\ten-us\n\t{\n\t\t#LOC_BOM_Test = Hello\n\t}\n}\n"
    bom_file.write_text(content, encoding="utf-8-sig")
    entries = parse_loc_file(bom_file)
    assert entries["#LOC_BOM_Test"] == "Hello"
