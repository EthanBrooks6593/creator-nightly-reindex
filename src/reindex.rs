use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::infrai::{Infrai, InfraiError, Vector};

#[derive(Debug, Clone, Deserialize)]
pub struct CreatorAsset {
    pub asset_id: String,
    pub creator: String,
    pub delivery_page: String,
    pub published: bool,
    pub subscriber_delivery: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IndexDecision {
    Index,
    SkipDraft,
    SkipPrivateDelivery,
}

pub fn index_decision(asset: &CreatorAsset) -> IndexDecision {
    if !asset.published {
        IndexDecision::SkipDraft
    } else if !asset.subscriber_delivery {
        IndexDecision::SkipPrivateDelivery
    } else {
        IndexDecision::Index
    }
}

#[derive(Debug, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub skipped: usize,
    pub collection: String,
}

pub async fn refresh_creator_index(
    infrai: &Infrai,
    collection: &str,
    assets: &[CreatorAsset],
) -> Result<ReindexReport, InfraiError> {
    let mut vectors = Vec::new();
    let mut skipped = 0;

    for asset in assets {
        if index_decision(asset) != IndexDecision::Index {
            skipped += 1;
            continue;
        }
        let page = infrai.scrape(&asset.delivery_page).await?;
        let embedding = infrai.embed(&page).await?;
        vectors.push(Vector {
            id: asset.asset_id.clone(),
            values: embedding,
            metadata: json!({
                "creator": asset.creator,
                "delivery_page": asset.delivery_page,
                "content_state": "published",
                "subscriber_delivery": true
            }),
        });
    }

    if !vectors.is_empty() {
        infrai.upsert(collection, &vectors).await?;
    }
    Ok(ReindexReport {
        indexed: vectors.len(),
        skipped,
        collection: collection.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(published: bool, subscriber_delivery: bool) -> CreatorAsset {
        CreatorAsset {
            asset_id: "asset-2048".into(),
            creator: "Mina Studio".into(),
            delivery_page: "https://creator.example/assets/asset-2048".into(),
            published,
            subscriber_delivery,
        }
    }

    #[test]
    fn indexes_only_published_subscriber_deliveries() {
        assert_eq!(index_decision(&asset(true, true)), IndexDecision::Index);
        assert_eq!(
            index_decision(&asset(false, true)),
            IndexDecision::SkipDraft
        );
        assert_eq!(
            index_decision(&asset(true, false)),
            IndexDecision::SkipPrivateDelivery
        );
    }
}
