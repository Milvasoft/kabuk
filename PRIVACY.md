# Kabuk privacy policy

Last updated: 17 October 2026

Kabuk is a desktop shell for Windows made by Milvasoft. This page says what Kabuk does with your data. In short: **Kabuk collects nothing, has no accounts and sends nothing about you anywhere.** Everything it knows stays on your computer.

## What stays on your computer

To do its work Kabuk reads, on your computer only:

- your windows (their titles, programs and places), to arrange them and show them on its bar;
- your apps, pinned apps, jump lists and browser bookmarks, for its bar and its launcher's search;
- how many notifications you have, for its bar;
- what is playing, the volume, the battery, the network and the processor and memory use, for its bar.

It writes its settings (`%APPDATA%\kabuk\kabuk.toml`), its state and its logs (`%LOCALAPPDATA%\kabuk`) to your computer. The logs say what Kabuk did, for finding faults; they stay on your computer and are never sent. While Kabuk runs it may set Windows' cursors, screen saver, wallpaper and colours to follow its look (each can be turned off in its settings), and puts them back when it exits.

## When Kabuk uses the internet

Kabuk goes on the internet only when you ask it to:

- **Check for updates** (the copy from GitHub; not the copy from the Microsoft Store, which the Store updates): when you press the button in Settings › About, Kabuk asks GitHub (`api.github.com`) for its latest release. Nothing about you is sent; GitHub sees the request as it sees any visit (see GitHub's privacy statement).
- **Create** (making a wallpaper with an AI model): only if you connect a provider yourself. With Claude (Anthropic) or OpenAI, the text you write is sent to that provider with your own API key, under that provider's privacy policy. Your key is kept in Windows' Credential Manager on your computer and is removed when you disconnect. With Ollama, everything stays on your computer.

Kabuk has no analytics, no telemetry, no advertising and no tracking.

## The Microsoft Store

If you installed Kabuk from the Microsoft Store, Microsoft handles the install, the updates and any data the Store itself collects, under Microsoft's privacy statement. Kabuk gets nothing from it.

## Children

Kabuk collects no data from anyone, children included.

## Changes and contact

If this policy changes, the new one is published here with its date. Questions: open an issue at <https://github.com/Milvasoft/kabuk/issues>.
