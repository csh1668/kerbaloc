"""Tests for the classifier."""

from ksp_translator.classifier import Category, classify, estimate_tokens


def test_skip_proper_noun():
    assert classify("#LOC_MyMod_Name^N", "Hybrid Engine") == Category.SKIP


def test_skip_numeric():
    assert classify("#LOC_MyMod_Val", "100 kN") == Category.SKIP


def test_skip_pure_format():
    assert classify("#LOC_MyMod_Fmt", "%s") == Category.SKIP


def test_skip_empty():
    assert classify("#LOC_MyMod_Empty", "") == Category.SKIP
    assert classify("#LOC_MyMod_Space", "   ") == Category.SKIP


def test_translate_short_text():
    assert classify("#LOC_MyMod_Short", "OK") == Category.TRANSLATE
    assert classify("#LOC_MyMod_Label", "Click to activate") == Category.TRANSLATE


def test_translate_long_text():
    long_text = "This is a very long description that contains many words and should be translated"
    assert classify("#LOC_MyMod_Desc", long_text) == Category.TRANSLATE


def test_skip_pure_tags():
    assert classify("#LOC_Test", "<<1>><<2>>") == Category.SKIP


def test_translate_text_with_tags():
    assert classify("#LOC_Test", "Thrust: <<1>>") == Category.TRANSLATE


def test_estimate_tokens():
    assert estimate_tokens("hello") == 1
    assert estimate_tokens("a" * 100) == 25
    assert estimate_tokens("") == 1  # minimum 1
