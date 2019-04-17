# Opus-like VOIP Traffic Generator

Client/server application designed to generate (contentless) traffic with similar flow characteristics to VOIP traffic experienced by [Discord](https://discord.gg).
Traffic is generated using traces (as recorded by [Felyne](https://github.com/felixmcfelix/felyne-bot)).

## Features
 * Configurable call duration (fixed, bounded, randomised).
 * Serverside batching of clients into rooms.
 * Optionally many streams per client.