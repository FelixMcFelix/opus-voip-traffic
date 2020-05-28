# Opus-like VOIP Traffic Generator

Client/server application designed to generate (contentless) traffic with similar flow characteristics to VOIP traffic experienced by [Discord](https://discord.gg).
Traffic is generated using traces (as recorded by [Felyne](https://github.com/felixmcfelix/felyne-bot)).

## Features
 * Configurable call duration (fixed, bounded, randomised).
 * Serverside batching of clients into rooms.
 * Optionally many streams per client.
 
## Attribution
If you use this in your work, please cite the parent work and include a link to this repository:
```bib
@article{DBLP:journals/tnsm/SimpsonRP20,
  author    = {Kyle A. Simpson and
               Simon Rogers and
               Dimitrios P. Pezaros},
  title     = {Per-Host DDoS Mitigation by Direct-Control Reinforcement Learning},
  journal   = {{IEEE} Trans. Network and Service Management},
  volume    = {17},
  number    = {1},
  pages     = {103--117},
  year      = {2020},
  url       = {https://doi.org/10.1109/TNSM.2019.2960202},
  doi       = {10.1109/TNSM.2019.2960202},
  timestamp = {Thu, 09 Apr 2020 21:56:10 +0200},
  biburl    = {https://dblp.org/rec/journals/tnsm/SimpsonRP20.bib},
  bibsource = {dblp computer science bibliography, https://dblp.org}
}
```
