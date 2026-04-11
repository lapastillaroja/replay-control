---
title: "Privacy Policy"
description: "Learn how we collect, use, and protect your personal data, what rights you have over your information, and how to contact us with questions."
summary: ""
date: 2023-09-07T17:19:07+02:00
lastmod: 2026-02-16T15:33:59+01:00
draft: false
type: "legal"
params:
  seo:
    title: "" # custom title (optional)
    description: "" # custom description (recommended)
    canonical: "" # custom canonical URL (optional)
    robots: "" # custom robot tags (optional)
---

## Summary

Replay Control does not collect or transmit any personal data. All your data (game library, settings, favorites) stays on your device. The only external network calls are to fetch game metadata and thumbnail resources when you explicitly request them, and to send an optional anonymous usage ping (see below).

## Anonymous Usage Statistics

By default, Replay Control sends a small anonymous ping once per day containing:

| Field | Example | Purpose |
|-------|---------|---------|
| Random install ID | `550e8400-...` | Distinguish unique installs (not tied to you) |
| App version | `0.3.1` | Track version distribution |
| CPU architecture | `aarch64` | Know what platforms are in use |
| Update channel | `stable` | Understand channel distribution |

The random install ID is a UUID generated on first startup. It is not derived from your hardware, network, or identity. You can reset it at any time by deleting the `install_id` line from `.replay-control/settings.cfg`.

**What is NOT collected:** IP addresses (not stored by our server), MAC addresses, hardware serials, hostnames, game library contents, usage patterns, page views, location data, or any other personal information.

**How to disable:** Go to Settings and toggle "Anonymous usage statistics" off. When disabled, no data is sent and no install ID is generated or stored.

**Where is data stored:** On Cloudflare Workers infrastructure. Our server code does not store IP addresses. Cloudflare's edge infrastructure may temporarily log IP addresses for up to 72 hours as part of standard abuse detection — this is the same exposure as the existing auto-update check (which contacts GitHub) and any other HTTPS request routed through a CDN. Only aggregate statistics are used.

**Why:** This data helps us understand how many installations exist and how quickly updates are adopted. Without it, the only signal is GitHub download counts, which cannot distinguish unique installs from repeated downloads.

**Source code:** The analytics client code is [open source](https://github.com/lapastillaroja/replay-control). You can verify exactly what is sent.

## This Website

This documentation website is hosted on [GitHub Pages](https://pages.github.com/). GitHub may collect standard web server logs as described in [GitHub's Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement). We do not add any additional tracking or analytics.

## Contact

For questions about this privacy policy, open an issue on [GitHub](https://github.com/lapastillaroja/replay-control/issues).

*Last updated: April 10, 2026*
