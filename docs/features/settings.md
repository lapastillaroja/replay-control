# Preferences

How user preferences work — the settings that customize your app experience without affecting the Pi.

{{< screenshot "more-page-mobile.png" "More page with preferences" >}}

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

The app ships with 11 built-in skins that control the color scheme of the web UI: REPLAY (default), MEGA TECH, PLAY CHOICE, ASTRO, SUPER VIDEO, MVS, RPG, FANTASY, SIMPLE PURPLE, METAL, and UNICOLORS. Each skin defines a full color palette — background, surface, text, accent, and border colors — applied via CSS custom properties.

{{< screenshot "skins-page-mobile.png" "Skin selection page" >}}

Browse and apply skins from **More > Skin**. The skin page shows all available skins with color previews, the currently active skin marked, and a one-tap apply.

### Sync Mode vs Manual Override

- **Sync mode** (default) -- the app follows the skin active on the RePlayOS TV interface. When someone changes the skin on the TV (via the RePlayOS menu), the web UI updates to match automatically — and vice versa. This keeps the TV and companion app visually consistent.
- **Manual override** -- pick a specific skin for the web UI, independent of the TV skin. Useful if you prefer a different color scheme on your phone or tablet than what is displayed on the TV.

A toggle at the top of the skin page controls the mode.

### Live Push to All Browsers

Skin changes are broadcast instantly to all connected browsers via server-sent events (SSE). If you change the skin from your phone, every other open tab or device sees the new theme immediately — no refresh needed. This also applies when the skin changes on the TV side in sync mode.

## GitHub API Key

Optional [GitHub](https://github.com/) personal access token for downloading thumbnails. Without a key, GitHub API requests are rate-limited to 60/hour; with one, the limit is 5,000/hour.

## Version Display

The app version and git hash are shown in the More page footer.
