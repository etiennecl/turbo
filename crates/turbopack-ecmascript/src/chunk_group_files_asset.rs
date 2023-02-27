use anyhow::Result;
use serde_json::Value;
use turbo_tasks::primitives::StringVc;
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    chunk::{
        Chunk, ChunkGroupVc, ChunkItem, ChunkItemVc, ChunkListReferenceVc, ChunkReferenceVc,
        ChunkVc, ChunkableAsset, ChunkableAssetVc, ChunkingContext, ChunkingContextVc,
    },
    ident::AssetIdentVc,
    reference::AssetReferencesVc,
};

use crate::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemContentVc,
        EcmascriptChunkItemVc, EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc,
        EcmascriptChunkPlaceablesVc, EcmascriptChunkVc, EcmascriptExports, EcmascriptExportsVc,
    },
    EcmascriptModuleAssetVc,
};

#[turbo_tasks::function]
fn modifier() -> StringVc {
    StringVc::cell("chunk group files".to_string())
}

/// An asset that exports a list of chunk URLs by putting the [asset] into a
/// ChunkGroup with the provided ChunkingContext.
#[turbo_tasks::value(shared)]
pub struct ChunkGroupFilesAsset {
    pub asset: ChunkableAssetVc,
    pub chunking_context: ChunkingContextVc,
    pub base_path: FileSystemPathVc,
    pub server_root: FileSystemPathVc,
    pub runtime_entries: Option<EcmascriptChunkPlaceablesVc>,
}

#[turbo_tasks::function]
fn chunk_list_modifier() -> StringVc {
    StringVc::cell("chunk list".to_string())
}

#[turbo_tasks::value_impl]
impl ChunkGroupFilesAssetVc {
    #[turbo_tasks::function]
    async fn chunk_group(self) -> Result<ChunkGroupVc> {
        let this = self.await?;
        let chunk_group =
            if let Some(ecma) = EcmascriptModuleAssetVc::resolve_from(this.asset).await? {
                ChunkGroupVc::from_chunk(ecma.as_evaluated_chunk(
                    this.chunking_context,
                    this.runtime_entries,
                    Some(self.chunk_list_path()),
                ))
            } else {
                ChunkGroupVc::from_asset(this.asset, this.chunking_context)
            };
        Ok(chunk_group)
    }

    #[turbo_tasks::function]
    async fn chunk_list_path(self) -> Result<FileSystemPathVc> {
        let this = &*self.await?;
        Ok(this.chunking_context.chunk_path(
            this.asset.ident().with_modifier(chunk_list_modifier()),
            ".json",
        ))
    }
}

#[turbo_tasks::value_impl]
impl Asset for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> AssetIdentVc {
        self.asset.ident().with_modifier(modifier())
    }

    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        unimplemented!()
    }

    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        unimplemented!()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    fn as_chunk(self_vc: ChunkGroupFilesAssetVc, context: ChunkingContextVc) -> ChunkVc {
        EcmascriptChunkVc::new(context, self_vc.as_ecmascript_chunk_placeable()).into()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    async fn as_chunk_item(
        self_vc: ChunkGroupFilesAssetVc,
        context: ChunkingContextVc,
    ) -> Result<EcmascriptChunkItemVc> {
        let this = self_vc.await?;
        Ok(ChunkGroupFilesChunkItem {
            context,
            inner: self_vc,
            chunk: this.asset.as_chunk(context),
        }
        .cell()
        .into())
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> EcmascriptExportsVc {
        EcmascriptExports::Value.cell()
    }
}

#[turbo_tasks::value]
struct ChunkGroupFilesChunkItem {
    context: ChunkingContextVc,
    inner: ChunkGroupFilesAssetVc,
    chunk: ChunkVc,
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for ChunkGroupFilesChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> ChunkingContextVc {
        self.context
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let chunks = self.inner.chunk_group().chunks();
        let mut data = Vec::new();
        let base_path = self.inner.await?.base_path.await?;
        for chunk in chunks.await?.iter() {
            let path = chunk.path().await?;
            if let Some(p) = base_path.get_path_to(&path) {
                data.push(Value::String(p.to_string()));
            }
        }
        Ok(EcmascriptChunkItemContent {
            inner_code: format!("__turbopack_export_value__({:#});\n", Value::Array(data)).into(),
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for ChunkGroupFilesChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> AssetIdentVc {
        self.inner.ident()
    }

    #[turbo_tasks::function]
    async fn references(&self) -> Result<AssetReferencesVc> {
        let chunk_group = self.inner.chunk_group();
        let chunks = chunk_group.chunks();

        let mut references: Vec<_> = chunks
            .await?
            .iter()
            .copied()
            .map(ChunkReferenceVc::new)
            .map(Into::into)
            .collect();

        // This creates the chunk list corresponding to this chunk group.
        references.push(
            ChunkListReferenceVc::new(
                self.inner.await?.server_root,
                chunk_group,
                self.inner.chunk_list_path(),
            )
            .into(),
        );

        Ok(AssetReferencesVc::cell(references))
    }
}
