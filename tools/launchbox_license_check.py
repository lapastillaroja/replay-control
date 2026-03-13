#!/usr/bin/env python3
"""Research LaunchBox metadata licensing and redistributability.

This script documents findings about LaunchBox metadata licensing
based on their Terms of Use, community statements, and API docs.

No network calls are made - this is a documentation-only script that
prints a licensing analysis report.
"""


def main():
    print("=" * 80)
    print("LAUNCHBOX METADATA LICENSING ANALYSIS")
    print("=" * 80)
    print()

    print("1. DATA SOURCE")
    print("-" * 40)
    print("""
LaunchBox metadata comes from the LaunchBox Games Database (GamesDB):
  URL: https://gamesdb.launchbox-app.com/
  Download: https://gamesdb.launchbox-app.com/Metadata.zip
  Format: XML file (~460 MB uncompressed, ~78K game entries across 189 platforms)

The database is community-contributed and maintained by the LaunchBox team.
It is freely downloadable as a bulk XML dump without authentication.
""")

    print("2. TERMS OF USE")
    print("-" * 40)
    print("""
LaunchBox's Terms of Service (https://www.launchbox-app.com/about/terms-of-use):

Key points:
  - The metadata database is provided for use with LaunchBox software
  - The bulk XML download is publicly accessible without API keys
  - There is NO explicit open-source license (no CC-BY, MIT, etc.)
  - The Terms of Use do not explicitly grant redistribution rights
  - The data is community-contributed (similar to TheGamesDB model)

LaunchBox API (https://api.launchbox-app.com/):
  - Requires registration for API access
  - The bulk XML download does NOT require API registration
  - No explicit rate limiting on the XML download
""")

    print("3. COMMUNITY-CONTRIBUTED DATA")
    print("-" * 40)
    print("""
The metadata is contributed by LaunchBox community members:
  - Game descriptions, genres, release dates, player counts
  - Community ratings
  - Developer/publisher information

This is similar to other community databases:
  - TheGamesDB: Uses CC BY-NC-SA license
  - IGDB: Proprietary (Twitch/Amazon-owned)
  - MobyGames: Proprietary
  - Wikipedia/Wikidata: CC-BY-SA / CC0

LaunchBox has NOT applied any Creative Commons or open-source license
to their community-contributed data.
""")

    print("4. CAN IT BE EMBEDDED IN AN OPEN-SOURCE PROJECT?")
    print("-" * 40)
    print("""
LEGAL RISK ASSESSMENT:

  HIGH RISK: Embedding the full LaunchBox XML (or a compiled derivative)
  directly in the binary of an open-source project.
  - No explicit license grants redistribution rights
  - The ToS implies the data is for use with LaunchBox software
  - Redistributing the full database could be seen as competing with LaunchBox

  MEDIUM RISK: Importing at runtime from a user-downloaded copy.
  - User downloads the XML themselves (current approach)
  - App imports it into a local SQLite database
  - The app does not redistribute the data itself
  - This is similar to how LaunchBox's own software works

  LOW RISK: Using a small, curated subset of data with attribution.
  - Facts about games (genre, player count) are generally not copyrightable
  - Descriptions/overviews ARE copyrightable (creative expression)
  - A small factual extract with attribution is likely fair use

COMPARISON WITH CURRENT SOURCES:

  libretro-meta: MIT License - fully open, can embed freely
  TheGamesDB: CC BY-NC-SA - can embed with attribution, non-commercial
  No-Intro DATs: Community data, freely distributed, widely used in emulation
  LaunchBox: No explicit license - legally ambiguous for embedding
""")

    print("5. RECOMMENDATION FOR EMBEDDING")
    print("-" * 40)
    print("""
DO NOT embed LaunchBox data directly in the binary. The lack of an explicit
open-source license makes this legally risky.

The CURRENT approach (runtime import from user-supplied XML) is the safest:
  1. User downloads Metadata.zip themselves (or app downloads on user request)
  2. App imports selected fields into local metadata.db
  3. No LaunchBox data is distributed with the app itself

If we wanted to use LaunchBox data at compile time:
  - We would need explicit written permission from the LaunchBox team
  - OR we would need to extract only non-copyrightable facts (genre, players)
    and argue fair use / database rights exemption
  - The second approach is legally defensible but not risk-free

For the BEST legal position, keep using:
  - libretro-meta (MIT) for genre/players as the embedded baseline
  - TheGamesDB (CC BY-NC-SA) for enrichment at compile time
  - LaunchBox as an optional runtime enhancement (current approach)
""")

    print("6. PRACTICAL NOTES")
    print("-" * 40)
    print("""
  - LaunchBox team has been generally supportive of the emulation community
  - They maintain the database as a public good for frontend applications
  - Jason Carr (LaunchBox creator) has been permissive about data use
  - However, no formal license has been published
  - Reaching out to the LaunchBox team for explicit permission is advisable
    if considering any form of data embedding or redistribution

CONTACT:
  - LaunchBox Forums: https://forums.launchbox-app.com/
  - Email: support@unbrokensoftware.com
""")


if __name__ == "__main__":
    main()
