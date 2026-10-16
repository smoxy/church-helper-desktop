# Privacy Policy

**Effective Date:** 2026-10-16

## Data Collection
Church Helper Desktop does not collect, store, or transmit any personal user data.

The only identifier the application ever sends is an **anonymous installation id**, described
below. It is not linked to any personal data.

## Anonymous installation id
* **What it is:** a random identifier (UUID version 4) generated locally the first time the
  application checks for updates, and stored in the application's own settings file
  (`settings.json`, key `install_id`). It is derived from nothing on your computer and contains
  no personal information: no name, e-mail, hardware id, IP-derived data, or usage data.
* **Purpose:** counting how many installations are active and which application versions they
  run, so that we know when it is safe to stop supporting an old version, and so that a faulty
  release can be paused or a critical fix prioritised.
* **When and where it is sent:** only together with the update check, as the `X-Install-Id`
  request header, and **only to our own update server** (`api.adventistyouth.it`). It is never
  sent to GitHub (the fallback update source) or to any other third party, and it is never
  attached to the download of the update itself.
* **What the server keeps:** the id, the application version, the platform (e.g.
  `windows-x86_64`) and the time of the last check. Installations not seen for more than
  **90 days** are deleted automatically. Only aggregated counts (per version / per platform)
  are ever looked at.
* **How to reset it:** delete the `install_id` key from `settings.json` (or reinstall the app):
  a new, unrelated id is generated on the next update check.

## Network Communications
This application communicates with the following external services only when requested by the user (e.g., during automatic background polling or manual downloads):
* **Adventist Youth API** (`api.adventistyouth.it`): Used to fetch the list of weekly materials (current and next week) and to check for application updates (see above).
* **GitHub** (`github.com`): Used to download application updates and, only when our update server is unreachable, to check for them (without the installation id).
* **Content Delivery Networks (AWS S3 / Youtube)**: Used to download the actual resource files (PDFs, Zips, Videos) initiated by the resource list.

No other telemetry or usage data is sent to third parties.
