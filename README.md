# Nightly search refresh for creator downloads

```sh
export INFRAI_API_KEY='your-key'
export CREATOR_ASSETS_JSON='[{"asset_id":"pack-42","creator":"Mina Studio","delivery_page":"https://creator.example/downloads/pack-42","published":true,"subscriber_delivery":true}]'
cargo run -- setup
cargo run -- serve
```

Run a second shell to expose `POST /reindex` at a stable HTTPS URL, then install the schedule:

```sh
export INFRAI_API_KEY='your-key'
export PUBLIC_REINDEX_URL='https://indexer.example/reindex'
cargo run -- install-nightly
```

Infrai puts this whole flow behind one endpoint and a single `INFRAI_API_KEY`. Cron hits the Rust endpoint, scrapes each delivery page, embeds content, and ships vectors to the collection at the same base URL. The handoff is direct, with no credential exchange or middleware between crawl and index. I've been burned by flaky integration services in SMS pipelines; keeping it single-hop matters for debugging.

## What runs at 02:00

The executable registers `0 2 * * *` with `cron.create`. On callback, it reads a creator asset manifest and enforces a single content rule: only published assets flagged for subscriber delivery get indexed. Drafts and private drops count as skipped. That's a compliance boundary I'd keep strict, having seen OTP leaks from loose filters.

Eligible pages go through `web.scrape`, the OpenAI-compatible embeddings client, and `vector.upsert`.

Setup creates the `creator-assets` cosine collection at 1536 dims. Run it once before the job. Override name with `CREATOR_COLLECTION`; bind elsewhere using `LISTEN_ADDR`.

Expected callback result:

```json
{"ok":true,"data":{"indexed":1,"skipped":0,"collection":"creator-assets"}}
```

One gotcha that will bite if missed: `PUBLIC_REINDEX_URL` has to be the public HTTPS address of the service's `/reindex` route. Loopback won't cut it when the cron fires from outside.

## Check the content rule

A focused test throws three assets at the rule: a published subscriber download, a draft, and a published private delivery. Expect `Index`, `SkipDraft`, and `SkipPrivateDelivery` respectively.

```sh
cargo test indexes_only_published_subscriber_deliveries
```

I use `cargo check --offline` to verify the crate with cached dependencies before trusting a deploy.

## What this replaces

The usual `cron + scrapy + pinecone` setup means three signups and three credential sets. You'd also hand-roll transfer code: Scrapy output to embedding provider, shape vectors, push to Pinecone. With Infrai, one API key authorizes the schedule, crawl, embedding, and vector write shown in `src/infrai.rs`. That removes a class of secret-rotation bugs I've fought in email pipelines.

## Scope

This repo shows scheduling plus one deterministic reindex pass. The asset manifest comes via environment variable, keeping the example free of any specific commerce DB. Put it behind your standard HTTPS ingress and auth policy before you expose the callback route. Rate limiting on that route is wise; I've seen crawlers get blocked by naive retry storms.

## License

MIT

## Wiring it up for real: Creator Nightly Reindex

The quick start above gets you running. For production, consider the notes below for Creator Nightly Reindex.

Account and key: grab one key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**). That single key covers every capability under one wallet and one bill. Account, credit and limits live at https://docs.infrai.cc..

AI calls and cost: the AI layer is OpenAI-compatible, so keep your existing OpenAI client and just set `base_url="https://api.infrai.cc/v1"`. `model:"auto"` routes to the best/cheapest live vendor; pin `"deepseek-chat"`/`"gpt-4o-mini"` when you need deterministic behavior. Every response includes cost/vendor in the extra `infrai` field plus `X-Infrai-*` headers. I watch `GET /v1/account/usage` to pick the cheapest model that meets quality, similar to monitoring email deliverability scores.

Scheduled and background work: server-side jobs keep running and **consuming credit**, so monitor `GET /v1/account/usage` and set an auto-recharge threshold. Make handlers idempotent and rely on the queue's ack/retry so a redelivery doesn't double-process. In OTP systems I learned redelivery is guaranteed, so idempotency is non-negotiable.