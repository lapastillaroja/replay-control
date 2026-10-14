"""Regression tests for the offline Flycast catalog sync parser."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import sync_flycast_games as sync  # noqa: E402


CYCRAFT = r'''
const Game Games[] = {
    // A Naomi 2 set whose MAME driver file is sega/naomi.cpp.
    {
        "inidv3cy", nullptr, "Initial D // Cycraft", 0x4000, 0,
        "naomi2", GD, ROT0,
        { { "317-0406-com.pic", 0, 0x4000, 0xfe91a7af } },
        "gds-0039b"
    },
    {
        "inidv3ca", "inidv3cy", "Initial D Cycraft (Rev A)", 0x4000, 0,
        "naomi2", GD, ROT0,
        { { "317-0406-com.pic", 0, 0x4000, 0xfe91a7af } },
        "gds-0039a"
    },
    { nullptr }
};
'''


class FlycastSyncTests(unittest.TestCase):
    def test_cycraft_bios_overrides_ambiguous_mame_driver(self):
        games = sync.parse_flycast(CYCRAFT, minimum_games=2)
        self.assertEqual(games["inidv3cy"]["board"], "sega_naomi_2")
        self.assertEqual(games["inidv3ca"]["parent"], "inidv3cy")
        self.assertEqual(games["inidv3cy"]["display_name"], "Initial D // Cycraft")

    def test_duplicate_identical_game_is_accepted(self):
        first_game = next(sync.brace_blocks(sync.strip_comments(CYCRAFT)))
        duplicate = CYCRAFT.replace("    { nullptr }", "    {" + first_game + "},\n    { nullptr }")
        # The real upstream table contains a repeated shors2k2 definition.
        games = sync.parse_flycast(duplicate, minimum_games=2)
        self.assertEqual(len(games), 2)

    def test_unknown_bios_fails_instead_of_mislabeling(self):
        with self.assertRaisesRegex(ValueError, "unknown Flycast BIOS"):
            sync.parse_flycast(CYCRAFT.replace('"naomi2"', '"new-board"'), minimum_games=2)

    def test_merge_preserves_supplemental_metadata(self):
        games = sync.parse_flycast(CYCRAFT, minimum_games=2)
        existing = {"inidv3cy": {"display_name": "Official title", "year": "2006", "category": "Driving"}}
        mame = {"inidv3ca": {"year": "2004", "manufacturer": "Sega", "players": "2", "status": "preliminary"}}
        rows = {row["rom_name"]: row for row in sync.merge(games, existing, mame)}
        self.assertEqual(rows["inidv3cy"]["display_name"], "Official title")
        self.assertEqual(rows["inidv3cy"]["category"], "Driving")
        self.assertEqual(rows["inidv3ca"]["year"], "2004")
        self.assertEqual(rows["inidv3ca"]["board"], "sega_naomi_2")
        self.assertEqual(rows["inidv3ca"]["is_clone"], "true")


if __name__ == "__main__":
    unittest.main()
