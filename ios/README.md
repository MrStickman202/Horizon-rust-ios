# Horizon for iOS / iPadOS

A port of Horizon (Minecraft: Java Edition 26.3 in Rust) to iPhone and iPad, sideloaded like Amethyst.
Ahead-of-time compiled Rust: no JIT needed.

## Microsoft sign-in

The app's Azure app (public client, personal Microsoft accounts only, device-code flow):

- Client ID: `928164ca-93c9-4361-82d2-c740023f96f0`

It is not a secret: it ships inside every copy of the app. It is passed to the build as
`HORIZON_MSA_CLIENT_ID` (see `ios/config.env`). Sign-in uses the `consumers` endpoint; no tenant ID
and no client secret are used. Each player's tokens stay in their own device's Keychain.

Minecraft API access needs Mojang's approval of this client ID (https://aka.ms/mce-reviewappid);
until then sign-in stops at the Minecraft step with HTTP 403.

## Privacy

No player data, prepared game files, accounts or tokens are ever committed or bundled: the .ipa is
built by GitHub Actions from a clean checkout, and `tools/check_public_tree.py` rejects Mojang files.
