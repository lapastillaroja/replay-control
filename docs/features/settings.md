# Preferences

How user preferences work — the settings that customize your app experience without affecting the Pi.

## Overview

User preferences are accessible from the **More** page and control how the app displays information, which regional variants you prefer, and how it communicates with external services.

## Region Preference

Primary and secondary preferred ROM region. Options: USA, Europe, Japan, World (default: World). Affects:

- Sort order within system ROM lists (preferred region first)
- Search scoring (preferred region gets a bonus)
- Recommendation deduplication (picks the preferred-region variant when multiple exist)

## Language Preference

Primary and secondary language (e.g., English, Spanish, Japanese). Defaults based on your region preference. Used for sorting game manual search results by language relevance.

## Font Size

Normal or large text. Applied across the entire app.

## Skin / Theme

Browse and apply skins from the [RePlayOS](https://www.replayos.com/) skin collection. Two modes:

- **Sync mode** (default) -- the app matches the skin active on the TV interface. Changes on either side are reflected immediately across all connected browsers.
- **Manual override** -- pick a specific skin for the web UI, independent of the TV skin.

## GitHub API Key

Optional [GitHub](https://github.com/) personal access token for downloading thumbnails. Without a key, GitHub API requests are rate-limited to 60/hour; with one, the limit is 5,000/hour.

## Version Display

The app version and git hash are shown in the More page footer.
