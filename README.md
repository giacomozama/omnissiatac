![OmnissiATAC](/logo.png)

A Discord bot written in Rust to be used by 10 people or so.

Most of it is vibe-coded with the crappiest, cheapest, most heavily subsidized AI models Google can offer, the rest is manually coded. Not a cent was spent on tokens to make this, it is pure pauper slop. This README was written by a human.

Pretty much everything here is inside jokes. You should probably not be using this, although it works as far as I'm concerned.

## Features

- Plays music via Lavalink, it even supports playlists.
- Can say stuff via text-to-speech
- Can be conversed with via ollama
- Can generated visual AI slop via ComfyUI
- Has a fancy WebUI 

## Dependencies

- Rust >= 2024 (at build time)
- libopus
- Lavalink
- ollama for chat LLM features (optional)
- ComfyUI for the generating AI slop feature (optional)

## Building

- `cargo build --release`
- Copy `target/release/omnissiatac`, `target/release/omnissiatac-bot` and `target/release/config.example.toml` to a directory somewhere.

## Running

- Give both `omnissiatac` and `omnissiatac-bot` execution permissions.
- Edit `config.example.toml` with your config. Rename it to `config.toml`.
- Start Lavalink.
- Run `omnissiatac`.
- If it doesn't work at this point, I'm sorry, you're on your own.
