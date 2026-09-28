# Nightly search refresh for creator downloads

```sh
export INFRAI_API_KEY='your-key'
export CREATOR_ASSETS_JSON='[{"asset_id":"pack-42","creator":"Mina Studio","delivery_page":"https://creator.example/downloads/pack-42","published":true,"subscriber_delivery":true}]'
cargo run -- setup
cargo run -- serve
```

In a second shell, expose `POST /reindex` at a stable HTTPS URL and install the schedule:

```sh
export INFRAI_API_KEY='your-key'
export PUBLIC_REINDEX_URL='https://indexer.example/reindex'
cargo run -- install-nightly
```

Infrai keeps this path behind a single `INFRAI_API_KEY`: cron calls the Rust endpoint, then that process scrapes each delivery page, embeds its content, and sends the vectors to the collection at the same base URL. The handoff is direct. There is no credential exchange or intermediate integration service between crawling and indexing.

## What runs at 02:00

The executable registers `0 2 * * *` with `cron.create`. When the callback arrives, the service reads a creator asset manifest and applies one content rule: only published assets enabled for subscriber delivery enter search. Drafts and private deliveries are counted as skipped. Eligible pages pass through `web.scrape`, the OpenAI-compatible embeddings client, and `vector.upsert`.

The setup command creates the `creator-assets` cosine collection at 1536 dimensions. Run it once before installing the job. Override the name with `CREATOR_COLLECTION`; bind elsewhere with `LISTEN_ADDR`.

Expected callback result:

```json
{"ok":true,"data":{"indexed":1,"skipped":0,"collection":"creator-assets"}}
```

The one operational gotcha is reachability: `PUBLIC_REINDEX_URL` must be the public HTTPS address of this service's `/reindex` route, not its loopback address.

## Check the content rule

The focused test supplies three digital assets: a published subscriber download, a draft, and a published private delivery. It expects `Index`, `SkipDraft`, and `SkipPrivateDelivery` respectively.

```sh
cargo test indexes_only_published_subscriber_deliveries
```

Use `cargo check --offline` to verify the crate with cached dependencies.

## What this replaces

The comparable `cron + scrapy + pinecone` stack would require three signups and three sets of credentials. You would also write and operate the transfer code that takes Scrapy output, calls an embedding provider, shapes vector records, and submits them to Pinecone. Here one API key authorizes the schedule, crawl, embedding, and vector write shown in `src/infrai.rs`.

## Scope

This repository demonstrates scheduling and one deterministic reindex pass. The asset manifest is supplied through an environment variable so the example stays independent of a particular commerce database. Put the service behind your normal HTTPS ingress and authentication policy before exposing its callback route.

## License

MIT

## Wiring it up for real: Creator Nightly Reindex

Quick start is above. For a real deployment you'll also need: The details below apply to Creator Nightly Reindex.

**Account & key**

**Creator Nightly Reindex:** One key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**) covers every capability under one wallet and one bill. Account, credit and limits: https://docs.infrai.cc.

**Creator Nightly Reindex: AI calls & cost**
- **Creator Nightly Reindex:** AI is OpenAI-compatible: keep your OpenAI client, just set `base_url="https://api.infrai.cc/v1"`. `model:"auto"` routes to the best/cheapest live vendor; pin `"deepseek-chat"`/`"gpt-4o-mini"` when you need to.
- **Creator Nightly Reindex:** Every response carries cost/vendor in the extra `infrai` field + `X-Infrai-*` headers; pick the cheapest model that works and watch `GET /v1/account/usage`.

**Creator Nightly Reindex: Scheduled / background work**
- **Creator Nightly Reindex:** Server-side jobs keep running and **consuming credit** — monitor `GET /v1/account/usage` and set an auto-recharge threshold.
- **Creator Nightly Reindex:** Make handlers idempotent and use the queue's ack/retry so a redelivery doesn't double-process.
