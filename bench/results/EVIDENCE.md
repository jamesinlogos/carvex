# Evidence manifest (H-32)

This file is the SHA-256 manifest for artefacts cited by
`docs/VALIDATION.md`. It was generated on 2026-11-22 by hashing the
files listed below; no lockbox dataset, lockbox result, benchmark or
published figure was regenerated. Historical figures without a retained
raw artefact are listed at the end as **author-reported** and are labelled
that way in `docs/VALIDATION.md`.

## Bundle

- Archive: `bench/results/evidence-bundle-0.2.0.tar.gz`
- Archive SHA-256: `103d738118a9d8c718ff5d2935c850c121a192ee7051336ed6e956f0db10de2f`
- Manifest: `bench/results/EVIDENCE.sha256`
- Manifest SHA-256: `95f923b1ada74061eeac5f34d64f65c790e8a994d028ff6f2fc813a66524cd4d`
- Verification: extract the archive into an empty directory, then run
  `sha256sum -c bench/results/EVIDENCE.sha256` from that directory. All 47
  listed files were verified this way before the bundle was committed.
- The 1.8 GiB lockbox dataset is **not** bundled; its `SHA256SUMS` and
  `lockbox.json` are bundled, along with the one-shot execution results.

Status values: `in-tree + bundle` means a tracked file that is also in the
archive; `bundle (ignored source)` means a locally retained artefact whose
source path is ignored by `.gitignore`; `public API + bundle` means a saved
public GitHub API response; `reconstructed + bundle` means a document
reconstruction, not an independent observation.

## Artefacts

| Path | VALIDATION § | SHA-256 | Status | Note |
|---|---|---|---|---|
| `bench/foremost.conf` | 1 | `a091c518d256b182ef0efa3320535f38102b14cdc7fc44903fc2966ab3df096d` | in-tree + bundle | frozen tool config |
| `bench/lockbox.log` | 6 | `da8b7d25ff410b92f5f1e8777d874223c57d611957e83be749a0de4dc323a3a3` | reconstructed + bundle | RECONSTRUCTED from VALIDATION.md; not independently observed |
| `bench/lockbox/SHA256SUMS` | 1, 6 | `a5724038df4d76bdf1691dc429c86292f58f41d15b19c3ab6cdab1b66446f68f` | bundle (ignored source) | frozen dataset checksum file; digest matches VALIDATION |
| `bench/lockbox/lockbox.json` | 1, 6 | `3c98caaa693791a2198bb2dfdabf9797c5e25a6328785042b299ab176148b93c` | bundle (ignored source) | frozen dataset manifest; digest matches VALIDATION |
| `bench/results/baseline-v0.1/SHA256SUMS` | 7 | `4cacd3683d1b90d2abb5a78982b8f2240b226679bba0486126e686bcb27fd6ed` | in-tree + bundle | frozen baseline checksum file |
| `bench/results/baseline-v0.1/results.json` | 7 | `96f2c268c027e638628bde67cce36b894bb6335dcb76c5bcd4a9aa724569c0af` | in-tree + bundle | frozen baseline figures |
| `bench/results/cfreds/SHA256SUMS` | 7 | `a234b30efcee1d50324018763c82e7393ec32f5d724fc84ea491dbb2b899d18c` | in-tree + bundle | frozen CFReDS checksum file |
| `bench/results/cfreds/cfreds-raw-report.json` | 7 | `808b532c138ed8c61d8cf68dec1b3e43c968dd9aac9f26a583c4e134e0006edf` | in-tree + bundle | CFReDS raw-mode figures |
| `bench/results/cfreds/cfreds-report.csv` | 7 | `4d1a197f88f1e8a08d475b549876b009427b80a595eee911018484d8a041af47` | in-tree + bundle | CFReDS comparison rows |
| `bench/results/cfreds/cfreds-report.json` | 7 | `875d0e91e97bccd8fa13b88a7a85aec30a68084857c95bd9667ce0d0076cc178` | in-tree + bundle | CFReDS comparison figures |
| `bench/results/ci_jobs_44548142275.json` | 3 | `f5fb873bc28e6444d96a190199e1e228da411612c62ca1810c959b0ead440e1f` | public API + bundle | public GitHub API job observations |
| `bench/results/ci_jobs_45035152791.json` | 3 | `a9e5c42fc0ff00baefa2daf260e2266dc5f184b1b50b3f9dc5233872126e0655` | public API + bundle | public GitHub API job observations |
| `bench/results/ci_run_44548142275.json` | 3 | `1eaa159d7f551e9e72c45bcef98ec281593156ea57c703575a8f15fbeffdc34b` | public API + bundle | public GitHub API run observation |
| `bench/results/ci_run_45035152791.json` | 3 | `c920c7b1862329a0ffbfa838bd384b6b6070920160bba6855951066e77d3722a` | public API + bundle | public GitHub API run observation |
| `bench/results/coverage/coverage.log` | 5 | `a564712a1f7c97a8f0bcbc30db67c3037ec8efe428d6855728892cea8f9ddc73` | bundle (ignored source) | latest committed coverage gate log |
| `bench/results/coverage/lcov.info` | 5 | `a5cc97b8a115d067997d2b13c0adb3a69e865a667d50142ceb3c5160779c92ae` | bundle (ignored source) | latest committed coverage report |
| `bench/results/d1_fragment_profile.json` | 2, 8 | `30b9735a981952a4d5c81920d0eb5bcaa4edd9d7ac15ef86b4329a892c3f0678` | in-tree + bundle | B-06 development profile |
| `bench/results/final-run.log` | 7 | `a1b2ca596ba4a26e4a74ef84ff3e52041b36becd767925dafb0f5779e3d2ee93` | bundle (ignored source) | frozen final run log |
| `bench/results/final/BENCHMARK.md` | 7 | `208974d2fdc9d25c85c7c5dbe792fb29c21dbf949a37c05bf57994ac2a9985c0` | in-tree + bundle | frozen published report |
| `bench/results/final/SHA256SUMS` | 7 | `9d958b0b35409c73c4473b4f16554b1f333f276a9033b838bfda99d66dfc88fd` | in-tree + bundle | frozen 0.1.0 result checksum file |
| `bench/results/final/results.csv` | 7 | `d3fda598265f004051be1ec35b30c19024963c26bc6107df580aee4257a0d073` | in-tree + bundle | frozen published rows |
| `bench/results/final/results.json` | 7 | `96f2c268c027e638628bde67cce36b894bb6335dcb76c5bcd4a9aa724569c0af` | in-tree + bundle | frozen published figures |
| `bench/results/full-0.2.0/BENCHMARK.md` | 7 | `f8434ed9d555038d2191616372674f72d186cde39dc2269686b2d14b50f0b9cd` | bundle (ignored source) | generated dev report |
| `bench/results/full-0.2.0/SHA256SUMS` | 7 | `01b6881bc42b8927a7ac600d5f484b17f942007c9545c74328858beb1f15844b` | bundle (ignored source) | result checksum file; digest matches VALIDATION |
| `bench/results/full-0.2.0/results.csv` | 7 | `2bd5edbccf8c232d9b0b15852c663e74aef0f5763ca1024f682e24fda0cd419b` | bundle (ignored source) | 60 run rows |
| `bench/results/full-0.2.0/results.json` | 7 | `24950b59c1d2406d28e9791ae698626624ed5a593e4614de0ca50ba796c3de08` | bundle (ignored source) | full dev benchmark table |
| `bench/results/full-run.log` | 7 | `798bdf2ec47ddfc4986b11a23033ad08cac19e0f7acffb808dfbb78c111ddbae` | bundle (ignored source) | full dev execution log |
| `bench/results/gate_h29.log` | 2 | `e2dc29ac6eb0b6deca0b733964dfe632635ffeac1314a1e7050f5bbc44d92b4d` | bundle (ignored source) | H-29 gate log; copied from the recorded local path |
| `bench/results/gate_h30_final.log` | 2, 4 | `6f14c34917499d28a98a0c40ca89d436fa7b904a488be2013d477cda7e89ad50` | bundle (ignored source) | H-30 full gate log; copied from the recorded local path |
| `bench/results/gate_h31_final.log` | 2 | `706a62a931edba937de960f3b2719b0038d6271eaf10ed309f34c954144d5f72` | bundle (ignored source) | H-31 full gate log; copied from the recorded local path |
| `bench/results/lockbox-run.log` | 6 | `fe2161489b632cac164cf96ef019b1a4ffeef77bc3613b2e72da4ac0c34fa358` | bundle (ignored source) | one-shot execution log |
| `bench/results/lockbox-v1/BENCHMARK.md` | 6 | `67cd1dd32cfa4aae2bee86106cb8a6c8026817ed7bcab60eeec9894ceeb6b8ea` | bundle (ignored source) | generated lockbox report |
| `bench/results/lockbox-v1/DATASET_SHA256SUMS` | 6 | `a5724038df4d76bdf1691dc429c86292f58f41d15b19c3ab6cdab1b66446f68f` | bundle (ignored source) | copy of frozen dataset checksum file |
| `bench/results/lockbox-v1/SHA256SUMS` | 6 | `9f8b4e5d475ccd38587f14abb127ef56b2d6b0c967eb1c07722487bb04f03734` | bundle (ignored source) | result checksum file; digest matches VALIDATION |
| `bench/results/lockbox-v1/results.csv` | 6 | `3bc56fe60b16ef6e14ed254ee5084995eff0db4596dd6dde6ff8faa54f9a8bb8` | bundle (ignored source) | 100 run rows |
| `bench/results/lockbox-v1/results.json` | 6 | `3d1fe4fa226bd6bf08c99c1f9e5bf9f4b240d1de9ddbe57ad0450b07a948d3aa` | bundle (ignored source) | pooled lockbox table |
| `bench/results/perf_h29.log` | 2 | `a5d02c55ad8760665d30f69b21b23ce1dabb63d79319a11559d1bbab51e59152` | bundle (ignored source) | H-29 perf log; copied from the recorded local path |
| `bench/results/status_red_before_docs.txt` | 2 | `b2516e08c968a28bafd6d6440929af71d8cb07a49358b65d3d4dba06289494da` | bundle (ignored source) | H-31 red control before reconciliation |
| `bench/results/status_red_wrong_label.txt` | 2 | `791b851727a7c9a6c79b4d70766e39eed8d9f01ed390e6ffda8ce055b22c8396` | bundle (ignored source) | H-31 red control with a deliberately wrong label |
| `bench/scalpel.conf` | 1 | `9c55a406f8911167c912b790c06fcb1c54247b59b32fecc4ef541c466df653e0` | in-tree + bundle | frozen tool config |
| `dist/carvex-linux-x86_64` | 1 | `b9e6c5f4d9cd5dd6ed633acda799e1e1c0880717de5030a5fe61a8ce5f7858af` | in-tree + bundle | 0.2.0 Linux binary; superseded by the 0.3.0 release rebuild |
| `dist/carvex-windows-x86_64.exe` | 1 | `503553ea17cd15522ce694ecb5eee0a8daacc8d6fed9ff434fe48262ad17fc9b` | in-tree + bundle | 0.2.0 Windows binary; packaged execution never claimed |
| `docs/BENCHMARK_PROTOCOL.md` | 1 | `d38607b86850312ec9906acd0dd732f965ca10cd5e78ff567a7076360b8008b6` | in-tree + bundle | protocol and freeze |
| `docs/baseline/harness_repro_v0.2.0.txt` | 2 | `a53690f3b5d6b4e907d3be8e821ebe07da6b6fd741f216095616b5b5738da1c1` | in-tree + bundle | X01-X24 control output |
| `docs/baseline/jpeg_splice_v0.2.0.txt` | 2 | `3608e3bce101adac187a686d925dd7d5590b4c6d36412c9ee21f98febd99c3b9` | in-tree + bundle | H-18 4,000-pair property output |
| `docs/baseline/repro_v0.1.0.txt` | 2 | `1ab236b9cf254e673fce6e9c5b2038c7e653773a36f27fde7a393081b2253bdf` | in-tree + bundle | red baseline |
| `scripts/repro_xfail.txt` | 2 | `e425fb47561da8569a9faec325f416fa23db8d34da35731a28ea38fc7475ac0f` | in-tree + bundle | xfail control list |

## Artefacts not retained

| Cited artefact / figure | VALIDATION § | SHA-256 | Label | Reason |
|---|---|---|---|---|
| `bench/results/fuzz/*.log` for the 18,520,025 and 235,272 historical executions | 4 | — | **author-reported** | The original fuzz logs were overwritten by later gate runs; current ignored logs are freshly generated and do not describe those runs. The H-30 and H-31 full-gate logs above do back their own execution counts. |
| Historical coverage percentages 69.72%, 68.51%, 67.50% and 63.42% | 5 | — | **author-reported** | The original lcov files were overwritten; only the latest `lcov.info` and gate logs are retained. |
| Early 80-test count and 0-vulnerability/91-crate audit summary | 2 | — | **author-reported** | No raw gate or audit log from that early run was retained. The 134-test count and H-30/H-31 gate results are backed by the gate logs above. |
| Remote GitHub Actions failure causes | 3 | — | **author-reported / not observed** | Public job metadata was retained, but the log endpoint returned `403 Must have admin rights to Repository.` No cause is claimed. |

No digest in this manifest was invented: every hash was computed from the
bundled file, and the quoted lockbox, full-benchmark, protocol, config and
dist digests in `docs/VALIDATION.md` were checked against those computed
values before this file was written.
