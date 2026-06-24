# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust tool to compute **DEC** and **FEC** (Brazilian PRODIST distribution-reliability
indices) for a generic bus network. The reference problem being validated against lives in
`references/` (`ex-decfec.jpg` + `resolucao-ex-decfec.md` + `regras-dec-fec.md`).

Code comments, doc-strings, RON comments, and the reference docs are in **Portuguese**, and
the user communicates in Portuguese — respond in Portuguese.

## Commands

```bash
cargo test                      # all tests (lib unit tests + tests/ integration)
cargo test sd1_checksums        # a single test by name substring
cargo test --test ref-exercise  # one integration test file
cargo clippy --all-targets      # lint (keep it clean — CI-grade expectation here)
cargo build

# Runner (the CLI has three modes; first arg is always the network .ron):
cargo run -- networks/ref-exercise.ron                                  # summary (Cc, buses, sources)
cargo run -- networks/ref-exercise.ron downstream 6                     # consumers a jusante da chave 6
cargo run -- networks/ref-exercise.ron dec-fec scenarios/item_a.ron 1   # DEC/FEC of the set downstream of switch "1"
```

`dec-fec` without a final switch arg reports over the whole system.

## Commits

When the user asks for a commit, do **not** run `git commit` — deliver only the commit
**message text**, in Conventional Commits format with a **succinct body**, written in
**English**. The user commits themselves.

Before handing over that text, run the validation gate and make sure all three pass (fix and
re-run if anything fails):

```bash
cargo test
cargo fmt --check
cargo clippy --workspace --all-targets
```

## Architecture

Library + binary: `src/lib.rs` exposes `topology` and `fault` (the reusable domain, intended
to back a future GUI too); `src/main.rs` is only the CLI runner over those modules.

### Topology (`src/topology.rs`) — the network as a graph

- **Buses are nodes (declared explicitly), switches and lines are edges.** `Element::Line {
  consumers }` carries a consumer block; `Element::Switch { normal: Open|Closed }` is a
  maneuverable switch with no load (`Open` = NA/tie, `Closed` = NF/sectionalizer).
- **Switches are edges on purpose.** A switch is a 2-terminal device, so every switch sits
  between two buses (hence the `c4u`/`c4`-style bus pairs). Modeling a switch as a *node*
  would break at junctions: opening it would disconnect all neighbors. As an edge, opening a
  switch cuts exactly one link — which is what makes fault isolation correct.
- `downstream_lines(switch)` is the bridge to the indices: "a jusante da chave X" = the line
  blocks that lose power if only that switch opens (computed in normal config). The set
  `downstream_lines("1")` of a feeder head is that feeder's `Cc`.
- `energized(conduz)` is a multi-source BFS over branches that conduct; it is reused both by
  `downstream_lines` (normal config) and by the simulator (live config + faulted branches).
- RON input with **closed-world validation**: any `from`/`to` not in `buses` is an error
  (catches typos), plus duplicate ids, self-loops, missing substation, disconnected buses.

### Fault model (`src/fault.rs`) — discrete-event simulation

The core design choice: you **do not** declare "who is affected". A `Scenario` is a timeline
of `Event { at_min, branch, action: Fault|Repair|Open|Close }`; `simulate()` replays it,
recomputing connectivity in each inter-event phase and recording each block's de-energized
intervals. Outages are *derived from the graph*, which is why simultaneous/overlapping faults
and already-reconfigured networks compose for free.

`SimResult::indicators(net, set)` then aggregates per block: an interruption counts only if
**≥ 3 minutes** (PRODIST `MOMENTARY_LIMIT_MIN`); `DEC = Σ(consumers·duration)/Cc` (hours),
`FEC = Σ(consumers·count)/Cc`. A block hit by two faults yields `FIC = 2` (so FEC can exceed 1).

**All internal times are in minutes**; DEC is converted to hours only at report time.

### Scenario conventions (not obvious from code)

- **"Transferir a carga a jusante da chave X" = `Open X` + `Close <NA>`.** Just closing the
  tie lets power leak through the rest of the island; you must script the sectionalizing
  open too. Reproducing the staged restoration depends on this.
- A **fault needs a branch to land on.** The faulted span is a `Line` (e.g. `tr_3_4`), even
  with 0 consumers — it is the physical conductor a `Fault` event targets and that gets
  isolated. Its consumers (if any) only return on `Repair`.
- Topology = what *exists*; Scenario = what *happens* to it (by branch id). They are separate
  RON files.

## The reference network (`networks/ref-exercise.ron`)

Currently **only the SD1 feeder** is encoded and verified; SD2/SD3 + real ties are TODO.
`tests/ref-exercise.rs` locks it against the answer key (`Cc=5400`, `downstream(3)=4500`,
`downstream(6)=1700`, and the end-to-end item (a) → `DEC≈2.33 h`, `FEC≈1.15`). When extending
the network, use these `downstream`-checksums to verify each feeder before trusting it.

Caveat in the file: `na6_far`/`na2_far` are marked `// TEMP` as `Substation` stand-ins for
SD2/SD3 so item (a)'s transfers have a source to feed from; they become junctions wired to the
real feeders once SD2/SD3 are encoded.
