use anyhow::{anyhow, Result};
use turbo_tasks::{primitives::StringVc, TryJoinIterExt, Value};
use turbo_tasks_env::ProcessEnvVc;
use turbo_tasks_fs::FileSystemPathVc;
use turbopack::ecmascript::EcmascriptModuleAssetVc;
use turbopack_core::{
    asset::Asset,
    chunk::{
        ChunkGroupVc, ChunkListReferenceVc, ChunkableAsset, ChunkableAssetVc, ChunkingContext,
    },
    reference_type::{EntryReferenceSubType, ReferenceType},
    resolve::{origin::PlainResolveOriginVc, parse::RequestVc},
};
use turbopack_dev_server::{
    html::DevHtmlAssetVc,
    source::{asset_graph::AssetGraphContentSourceVc, ContentSourceVc},
};
use turbopack_node::execution_context::ExecutionContextVc;

use crate::{
    next_client::context::{
        get_client_asset_context, get_client_chunking_context, get_client_compile_time_info,
        get_client_runtime_entries, ClientContextType,
    },
    next_config::NextConfigVc,
};

#[turbo_tasks::function]
pub async fn create_web_entry_source(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    entry_requests: Vec<RequestVc>,
    server_root: FileSystemPathVc,
    env: ProcessEnvVc,
    eager_compile: bool,
    browserslist_query: &str,
    next_config: NextConfigVc,
) -> Result<ContentSourceVc> {
    let ty = Value::new(ClientContextType::Other);
    let compile_time_info = get_client_compile_time_info(browserslist_query);
    let context = get_client_asset_context(
        project_path,
        execution_context,
        compile_time_info,
        ty,
        next_config,
    );
    let chunking_context = get_client_chunking_context(
        project_path,
        server_root,
        compile_time_info.environment(),
        ty,
    );
    let entries = get_client_runtime_entries(project_path, env, ty, next_config);

    let runtime_entries = entries.resolve_entries(context);

    let origin = PlainResolveOriginVc::new(context, project_path.join("_")).as_resolve_origin();
    let entries = entry_requests
        .into_iter()
        .map(|request| async move {
            let ty = Value::new(ReferenceType::Entry(EntryReferenceSubType::Web));
            Ok(origin
                .resolve_asset(request, origin.resolve_options(ty.clone()), ty)
                .primary_assets()
                .await?
                .first()
                .copied())
        })
        .try_join()
        .await?;

    let chunk_groups_with_references: Vec<_> = entries
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, module)| async move {
            if let Some(ecmascript) = EcmascriptModuleAssetVc::resolve_from(module).await? {
                let chunk_list_path = chunking_context
                    .chunk_path(module.ident().with_modifier(chunk_list_modifier()), ".json");
                let chunk = ecmascript.as_evaluated_chunk(
                    chunking_context,
                    (i == 0).then_some(runtime_entries),
                    Some(chunk_list_path),
                );
                let chunk_group = ChunkGroupVc::from_chunk(chunk);
                let additional_references =
                    vec![
                        ChunkListReferenceVc::new(server_root, chunk_group, chunk_list_path).into(),
                    ];
                Ok((chunk_group, additional_references))
            } else if let Some(chunkable) = ChunkableAssetVc::resolve_from(module).await? {
                // TODO this is missing runtime code, so it's probably broken and we should also
                // add an ecmascript chunk with the runtime code
                Ok((
                    ChunkGroupVc::from_chunk(chunkable.as_chunk(chunking_context)),
                    vec![],
                ))
            } else {
                // TODO convert into a serve-able asset
                Err(anyhow!(
                    "Entry module is not chunkable, so it can't be used to bootstrap the \
                     application"
                ))
            }
        })
        .try_join()
        .await?;

    let (chunk_groups, additional_references): (Vec<_>, Vec<_>) =
        chunk_groups_with_references.into_iter().unzip();
    let additional_references = additional_references.into_iter().flatten().collect();

    let entry_asset = DevHtmlAssetVc::new(
        server_root.join("index.html"),
        chunk_groups,
        additional_references,
    )
    .into();

    let graph = if eager_compile {
        AssetGraphContentSourceVc::new_eager(server_root, entry_asset)
    } else {
        AssetGraphContentSourceVc::new_lazy(server_root, entry_asset)
    }
    .into();
    Ok(graph)
}

#[turbo_tasks::function]
fn chunk_list_modifier() -> StringVc {
    StringVc::cell("chunk list".to_string())
}
