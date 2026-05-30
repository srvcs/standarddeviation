# srvcs-standarddeviation

The standard-deviation service of the srvcs.cloud distributed standard library.

Its single concern: **the population standard deviation of a list of numbers**,
returned as an `f64`. It does no arithmetic of its own. It is a thin
orchestrator that delegates the entire computation to a single primitive:

```text
result = populationstddev(values).result    # one call to srvcs-populationstddev
```

So `standarddeviation([1,2,3,4,5]) ~= 1.4142135623730951`. Validation (e.g. an
**empty list**, or a non-numeric element) is propagated from
`srvcs-populationstddev`'s `422`.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Service identity, concern, and dependency list |
| `POST` | `/` | Compute the population standard deviation of the numbers in `values` |
| `GET` | `/healthz` `/readyz` `/metrics` `/openapi.json` | srvcs service standard surface |

```sh
curl -s -X POST localhost:8080/ -H 'content-type: application/json' -d '{"values": [1, 2, 3, 4, 5]}'
# {"values":[1,2,3,4,5],"result":1.4142135623730951}
```

Responses:

- `200 {"values": [...], "result": x}` — evaluated; `result` is an `f64`.
- `422` — empty list, or an element is not a valid number (forwarded from `srvcs-populationstddev`).
- `500` — a dependency returned an unusable response.
- `503` — a dependency is unavailable.

## Dependencies

- [`srvcs-populationstddev`](https://github.com/srvcs/populationstddev)

This service is an orchestrator: it never calls `srvcs-isnumber` directly.
Input validation propagates from its dependency — a non-numeric element is
caught by `srvcs-populationstddev`, whose `422` is forwarded verbatim.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SRVCS_BIND_ADDR` | `0.0.0.0:8080` | Bind address |
| `SRVCS_POPULATIONSTDDEV_URL` | `http://127.0.0.1:8090` | Base URL of `srvcs-populationstddev` |
| `SRVCS_ENV` | `development` | Environment label for logs |
| `RUST_LOG` | `info,tower_http=info` | Tracing filter |

## Local checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Orchestration tests stand up a mock dependency in-process that **actually
computes** the population standard deviation, so the composition is genuinely
exercised against asserted cases — e.g. `standarddeviation([1,2,3,4,5]) ~=
1.4142135623730951` — with a `1e-9` tolerance. See
[`srvcs/platform`](https://github.com/srvcs/platform) for the shared standard.

> Note: the `cargoHash` in `flake.nix` is inherited from the template and must be
> refreshed with a `nix build` before the Nix gates pass.
