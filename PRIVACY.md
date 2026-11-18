# Cove privacy policy

Effective November 18, 2026. This policy covers the Cove app. The website
[getcove.dev](https://getcove.dev) has its own [privacy policy](https://getcove.dev/privacy).

Cove is a desktop game launcher that runs on your computer. It has no account of its own, shows no ads
and sends no analytics or usage statistics. This page explains what Cove keeps on your computer, what it
sends to which services and why, and how cloud sync uses your own cloud storage.

## On your computer

- Your library, playtime, collections, wishlist, settings and save backups are stored in Cove's data
  folder on your computer and nowhere else.
- **Cove never sees or stores your passwords.** You sign in on each service's own page. The tokens Cove
  receives (stores, itch.io API key, cloud sync) are kept only in your operating system's keyring.
- Uninstalling Cove and deleting its data folder removes everything Cove stored. Tokens can also be
  removed in Cove's settings by signing out or disconnecting.

## Cove's catalogue proxy

Some features go through a small service run for Cove on Cloudflare Workers, so you need no API keys of
your own. It receives only what each feature needs:

- Catalogue: search words, filters and game ids (game data from IGDB, time to beat).
- Steam: your public SteamID and game ids, to read your Steam library and achievements with Cove's Steam
  Web API key.
- Prices: your country code and the titles on your wishlist (prices from IsThereAnyDeal).
- Google Drive sign-in: the sign-in code or refresh token, passed on to Google together with Cove's client
  secret. The answer is handed straight back to Cove; nothing is stored or cached.

The proxy has no database and no user accounts. To answer faster, it keeps answers in Cloudflare's cache
for a short time: catalogue data up to a week, the public Steam profile, library and achievements of a
SteamID for one hour, prices for three hours. Sign-in codes and tokens are never cached. Cloudflare
processes your IP address to deliver requests and to limit how many requests one address may send per
minute, and may record request metadata (such as the address called and the time) in its logs for a few
days. Request contents, including tokens, are not logged.

## Services Cove talks to directly

- **Stores you sign in to** (Steam, GOG, Epic Games, Amazon, itch.io): your library, downloads,
  achievements and cloud saves, with your own login.
- **Public information:** ProtonDB ratings and the Steam store search (Linux compatibility), Steam news of
  your games, free games from Epic, GOG and GamerPower, the Ludusavi manifest of save locations, and
  GE-Proton and umu-launcher releases from GitHub.
- **Updates:** Cove checks its releases on GitHub for a new version.
- **Discord** (only when you turn on the Discord status): the game you play goes to the Discord app on
  your computer, never over the network by Cove.

These services have their own privacy policies.

## Cloud sync of your saves

Cloud sync is off until you connect a cloud in Cove's settings. When you connect Google Drive, OneDrive
or Dropbox:

- **Cove only has access to its own app folder** in your cloud storage: Google Drive's hidden app data
  folder (scope `drive.appdata`), OneDrive's app folder (`Files.ReadWrite.AppFolder`) or a Dropbox
  "App folder". Cove cannot see, read or change your other files.
- Cove stores your games' save files there, together with a small list of the files, the name of the
  computer that saved them and the game's title. It also reads your account's e-mail address or name to
  show which account is connected.
- The data goes directly between your computer and your cloud provider. Nobody else, including the
  developers of Cove, can access it.
- To stop, disconnect the cloud in Cove's settings. You can delete Cove's folder in your cloud storage
  and revoke Cove's access in your Google, Microsoft or Dropbox account settings at any time.

Cove's use and transfer of information received from Google APIs adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy),
including the Limited Use requirements. Data from Google, Microsoft and Dropbox is used only to sync your
saves, is never sold, never used for advertising and never transferred to anyone else.

## Children

Cove is not directed at children and collects no personal data of its own.

## Changes

Changes to this policy are published in this file with a new effective date.

## Contact

Questions about privacy: open an issue on [GitHub](https://github.com/bjoernhar10/Cove-Launcher/issues),
or use the contact details on [getcove.dev](https://getcove.dev/privacy).
