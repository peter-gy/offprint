use offprint_browser::AttachedFrame;
use offprint_model::{ErrorStage, OffprintError, Result};
use serde_json::{Value, json};

use super::ChromiumPage;

fn frame_depth_in_tree(frame_tree: &Value, target_id: &str) -> Result<Option<u16>> {
    let root = frame_tree.get("frameTree").ok_or_else(|| {
        OffprintError::new(
            "offprint.frame.topology",
            ErrorStage::Collection,
            "frame tree response has no root",
        )
    })?;
    let mut pending = vec![(root, 0_u16)];
    while let Some((tree, depth)) = pending.pop() {
        let frame_id = tree
            .pointer("/frame/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.topology",
                    ErrorStage::Collection,
                    "frame tree entry has no frame identifier",
                )
            })?;
        if frame_id == target_id {
            return Ok(Some(depth));
        }
        for child in tree
            .get("childFrames")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let child_depth = depth.checked_add(1).ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.depth",
                    ErrorStage::Collection,
                    "frame depth exceeds the supported numeric range",
                )
            })?;
            let child_id = child
                .pointer("/frame/id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    OffprintError::new(
                        "offprint.frame.topology",
                        ErrorStage::Collection,
                        "child frame tree entry has no frame identifier",
                    )
                })?;
            if child_id == target_id {
                return Ok(Some(child_depth));
            }
            pending.push((child, child_depth));
        }
    }
    Ok(None)
}

impl ChromiumPage {
    pub async fn frame_owner_path(&self, frame: &AttachedFrame) -> Result<Vec<u32>> {
        let owner = self
            .prefer_frame_target_error(
                self.client
                    .command(
                        "DOM.getFrameOwner",
                        json!({"frameId": frame.target_id}),
                        Some(&frame.parent_session_id),
                    )
                    .await,
            )
            .await?;
        let backend_node_id = owner
            .get("backendNodeId")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.owner",
                    ErrorStage::Collection,
                    "frame owner response has no backend node identifier",
                )
            })?;
        let child_frame_tree = self
            .prefer_frame_target_error(
                self.client
                    .command("Page.getFrameTree", json!({}), Some(&frame.session_id))
                    .await,
            )
            .await?;
        let owner_frame_id = child_frame_tree
            .pointer("/frameTree/frame/parentId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.owner",
                    ErrorStage::Collection,
                    "attached frame has no owner document identifier",
                )
            })?
            .to_owned();
        let parent_frame_tree = self
            .prefer_frame_target_error(
                self.client
                    .command(
                        "Page.getFrameTree",
                        json!({}),
                        Some(&frame.parent_session_id),
                    )
                    .await,
            )
            .await?;
        let owner_depth =
            frame_depth_in_tree(&parent_frame_tree, &owner_frame_id)?.ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.owner",
                    ErrorStage::Collection,
                    "owner document is absent from its frame tree",
                )
            })?;
        let path_depth = owner_depth.checked_add(1).ok_or_else(|| {
            OffprintError::new(
                "offprint.frame.depth",
                ErrorStage::Collection,
                "frame depth exceeds the supported numeric range",
            )
        })?;
        let world = self
            .prefer_frame_target_error(
                self.client
                    .command(
                        "Page.createIsolatedWorld",
                        json!({
                            "frameId": owner_frame_id,
                            "worldName": "offprint-frame-owner",
                        }),
                        Some(&frame.parent_session_id),
                    )
                    .await,
            )
            .await?;
        let execution_context_id = world.get("executionContextId").cloned().ok_or_else(|| {
            OffprintError::new(
                "offprint.frame.owner",
                ErrorStage::Collection,
                "isolated frame owner world has no execution context",
            )
        })?;
        let resolved = self
            .prefer_frame_target_error(
                self.client
                    .command(
                        "DOM.resolveNode",
                        json!({
                            "backendNodeId": backend_node_id,
                            "executionContextId": execution_context_id,
                        }),
                        Some(&frame.parent_session_id),
                    )
                    .await,
            )
            .await?;
        let object_id = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.owner",
                    ErrorStage::Collection,
                    "frame owner could not be resolved in its isolated world",
                )
            })?;
        let response = self
            .client
            .command(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": r#"function(expectedDepth) {
                        const apply = Reflect.apply;
                        const querySelectorAll =
                            Document.prototype.querySelectorAll;
                        const nodeListItem = NodeList.prototype.item;
                        const path = new Array(expectedDepth);
                        let owner = this;
                        for (let level = expectedDepth - 1; level >= 0; level -= 1) {
                            const document = owner.ownerDocument;
                            const owners = apply(
                                querySelectorAll,
                                document,
                                ["iframe,frame"]
                            );
                            let index = -1;
                            for (let candidate = 0; candidate < owners.length; candidate += 1) {
                                if (apply(nodeListItem, owners, [candidate]) === owner) {
                                    index = candidate;
                                    break;
                                }
                            }
                            if (index < 0) return null;
                            path[level] = index;
                            if (level === 0) break;
                            const view = document.defaultView;
                            owner = view ? view.frameElement : null;
                            if (!owner) return null;
                        }
                        return path;
                    }"#,
                    "arguments": [{"value": path_depth}],
                    "returnByValue": true,
                    "silent": true,
                }),
                Some(&frame.parent_session_id),
            )
            .await;
        let _ignored = self
            .client
            .command(
                "Runtime.releaseObject",
                json!({"objectId": object_id}),
                Some(&frame.parent_session_id),
            )
            .await;
        let response = self.prefer_frame_target_error(response).await?;
        response
            .pointer("/result/value")
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<u32>>(value).ok())
            .filter(|path| path.len() == usize::from(path_depth))
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.frame.owner",
                    ErrorStage::Collection,
                    "frame owner path is absent from the isolated parent world",
                )
            })
    }
}
