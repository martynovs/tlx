# Pinned test oracle

`tla2tools.jar` is the exact TLC/SANY test dependency declared by
`scripts/dependencies.py`, retained because the upstream v1.8.0 asset was replaced.
It is not a tlx runtime dependency.

- Upstream source: https://github.com/tlaplus/tlaplus/tree/1813307068bbc5759a441df820f3f548948d6b7a
- Original distribution URL: https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
- SHA-256: `b490f45c1de08e4ff9753259a00338981b9cf464f01ca9e9cd5f19f33cf0bb92`

The upstream license is retained in `LICENSE.txt`. The jar also contains its bundled dependency
licenses and notices, including Commons Math and JLine. Preserve those notices when redistributing it.
`fetch-deps.sh` verifies both the checksum and the embedded source revision before installing it.
A corrupted bundled jar is rejected rather than replaced with arbitrary bytes from the download URL.

For spec tests and CI without the Examples corpus:

```sh
bash scripts/fetch-deps.sh --tests-only
```

The default command still fetches the Examples corpus for the corpus drivers.
