use pageknot_browser::AttachedFrame;
use pageknot_model::{ErrorStage, PageKnotError, Result};
use pageknot_protocol::VisualFallback;
use serde::Deserialize;
use serde_json::{Value, json};

use super::ChromiumPage;

const VIEWPORT_EPSILON: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
struct DocumentRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ViewportRect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrolledRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_width: f64,
    viewport_height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameOwnerPlacement {
    x: f64,
    y: f64,
    scale_x: f64,
    scale_y: f64,
    scroll_x: f64,
    scroll_y: f64,
}

#[derive(Debug, Deserialize)]
struct PositionedFallback {
    x: String,
    y: String,
    width: String,
    height: String,
}

impl ChromiumPage {
    pub(super) async fn position_visual_fallback(
        &self,
        session_id: &str,
        fallback: &VisualFallback,
    ) -> Result<VisualFallback> {
        let value = self
            .collector_call_in_session(session_id, "positionVisualFallback", json!([fallback.id]))
            .await?;
        let positioned: PositionedFallback = serde_json::from_value(value).map_err(|error| {
            PageKnotError::new(
                "pageknot.visual_fallback.protocol_shape",
                ErrorStage::Collection,
                format!("visual fallback position response is invalid: {error}"),
            )
        })?;
        Ok(VisualFallback {
            id: fallback.id.clone(),
            kind: fallback.kind,
            x: positioned.x,
            y: positioned.y,
            width: positioned.width,
            height: positioned.height,
        })
    }

    pub(super) async fn visual_fallback_clip(
        &self,
        session_id: &str,
        fallback: &VisualFallback,
    ) -> Result<ViewportRect> {
        let mut rect = DocumentRect {
            x: fallback_coordinate(&fallback.x, "x")?,
            y: fallback_coordinate(&fallback.y, "y")?,
            width: fallback_dimension(&fallback.width, "width")?,
            height: fallback_dimension(&fallback.height, "height")?,
        };
        let frames = self.targets.frames().await?;
        let mut current_session = session_id.to_owned();

        while current_session != self.session_id {
            let visible = self.scroll_document_rect(&current_session, rect).await?;
            let frame = frames
                .iter()
                .find(|frame| frame.session_id == current_session)
                .ok_or_else(|| {
                    PageKnotError::new(
                        "pageknot.visual_fallback.frame",
                        ErrorStage::Collection,
                        "visual fallback frame is no longer attached",
                    )
                })?;
            let owner = self.frame_owner_placement(frame).await?;
            rect = DocumentRect {
                x: owner.x + visible.x * owner.scale_x + owner.scroll_x,
                y: owner.y + visible.y * owner.scale_y + owner.scroll_y,
                width: visible.width * owner.scale_x,
                height: visible.height * owner.scale_y,
            };
            current_session.clone_from(&frame.parent_session_id);
        }

        let visible = self.scroll_document_rect(&self.session_id, rect).await?;
        Ok(ViewportRect {
            x: visible.x,
            y: visible.y,
            width: visible.width,
            height: visible.height,
        })
    }

    async fn scroll_document_rect(
        &self,
        session_id: &str,
        rect: DocumentRect,
    ) -> Result<ScrolledRect> {
        let context_id = self.root_isolated_world(session_id).await?;
        let response = self
            .client
            .command(
                "Runtime.callFunctionOn",
                json!({
                    "executionContextId": context_id,
                    "functionDeclaration": r#"async function(rect) {
                        const root = document.documentElement;
                        const body = document.body;
                        const viewportWidth = window.innerWidth;
                        const viewportHeight = window.innerHeight;
                        const maximumX = Math.max(
                            0,
                            root.scrollWidth,
                            body ? body.scrollWidth : 0
                        ) - viewportWidth;
                        const maximumY = Math.max(
                            0,
                            root.scrollHeight,
                            body ? body.scrollHeight : 0
                        ) - viewportHeight;
                        const targetX = Math.min(
                            Math.max(0, rect.x - Math.max(0, (viewportWidth - rect.width) / 2)),
                            Math.max(0, maximumX)
                        );
                        const targetY = Math.min(
                            Math.max(0, rect.y - Math.max(0, (viewportHeight - rect.height) / 2)),
                            Math.max(0, maximumY)
                        );
                        Reflect.apply(window.scrollTo, window, [targetX, targetY]);
                        await new Promise((resolve) => {
                            requestAnimationFrame(() => requestAnimationFrame(resolve));
                        });
                        return {
                            x: rect.x - window.scrollX,
                            y: rect.y - window.scrollY,
                            width: rect.width,
                            height: rect.height,
                            viewportWidth: window.innerWidth,
                            viewportHeight: window.innerHeight
                        };
                    }"#,
                    "arguments": [{
                        "value": {
                            "x": rect.x,
                            "y": rect.y,
                            "width": rect.width,
                            "height": rect.height,
                        }
                    }],
                    "returnByValue": true,
                    "awaitPromise": true,
                    "silent": true,
                }),
                Some(session_id),
            )
            .await?;
        let value = call_result_value(response, "scroll visual fallback into view")?;
        let visible: ScrolledRect = serde_json::from_value(value).map_err(|error| {
            PageKnotError::new(
                "pageknot.visual_fallback.protocol_shape",
                ErrorStage::Collection,
                format!("visual fallback viewport response is invalid: {error}"),
            )
        })?;
        if visible.x < -VIEWPORT_EPSILON
            || visible.y < -VIEWPORT_EPSILON
            || visible.x + visible.width > visible.viewport_width + VIEWPORT_EPSILON
            || visible.y + visible.height > visible.viewport_height + VIEWPORT_EPSILON
        {
            return Err(PageKnotError::new(
                "pageknot.visual_fallback.viewport",
                ErrorStage::Collection,
                "visual fallback does not fit inside its browser viewport",
            )
            .with_detail("width", visible.width)
            .with_detail("height", visible.height)
            .with_detail("viewportWidth", visible.viewport_width)
            .with_detail("viewportHeight", visible.viewport_height));
        }
        Ok(visible)
    }

    async fn frame_owner_placement(&self, frame: &AttachedFrame) -> Result<FrameOwnerPlacement> {
        let owner = self
            .client
            .command(
                "DOM.getFrameOwner",
                json!({"frameId": frame.target_id}),
                Some(&frame.parent_session_id),
            )
            .await?;
        let backend_node_id = owner
            .get("backendNodeId")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.visual_fallback.frame_owner",
                    ErrorStage::Collection,
                    "visual fallback frame owner has no backend node identifier",
                )
            })?;
        let child_tree = self
            .client
            .command("Page.getFrameTree", json!({}), Some(&frame.session_id))
            .await?;
        let owner_frame_id = child_tree
            .pointer("/frameTree/frame/parentId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.visual_fallback.frame_owner",
                    ErrorStage::Collection,
                    "visual fallback frame has no owner document",
                )
            })?;
        let world = self
            .client
            .command(
                "Page.createIsolatedWorld",
                json!({
                    "frameId": owner_frame_id,
                    "worldName": "pageknot-visual-fallback-owner",
                }),
                Some(&frame.parent_session_id),
            )
            .await?;
        let context_id = world.get("executionContextId").cloned().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.visual_fallback.frame_owner",
                ErrorStage::Collection,
                "visual fallback frame owner world has no execution context",
            )
        })?;
        let resolved = self
            .client
            .command(
                "DOM.resolveNode",
                json!({
                    "backendNodeId": backend_node_id,
                    "executionContextId": context_id,
                }),
                Some(&frame.parent_session_id),
            )
            .await?;
        let object_id = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.visual_fallback.frame_owner",
                    ErrorStage::Collection,
                    "visual fallback frame owner could not be resolved",
                )
            })?;
        let response = self
            .client
            .command(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": r#"async function() {
                        const apply = Reflect.apply;
                        const getBounds = Element.prototype.getBoundingClientRect;
                        const scrollIntoView = Element.prototype.scrollIntoView;
                        const chain = [];
                        let current = this;
                        while (current) {
                            chain.push(current);
                            const view = current.ownerDocument.defaultView;
                            current = view ? view.frameElement : null;
                        }
                        for (let index = chain.length - 1; index >= 0; index -= 1) {
                            apply(scrollIntoView, chain[index], [{
                                block: "center",
                                inline: "center"
                            }]);
                        }
                        await new Promise((resolve) => {
                            requestAnimationFrame(() => requestAnimationFrame(resolve));
                        });
                        current = this;
                        let bounds = apply(getBounds, current, []);
                        let elementScaleX =
                            current.offsetWidth > 0 ? bounds.width / current.offsetWidth : 1;
                        let elementScaleY =
                            current.offsetHeight > 0 ? bounds.height / current.offsetHeight : 1;
                        let x = bounds.left + current.clientLeft * elementScaleX;
                        let y = bounds.top + current.clientTop * elementScaleY;
                        let scaleX = elementScaleX;
                        let scaleY = elementScaleY;
                        let rootDocument = current.ownerDocument;
                        current = rootDocument.defaultView
                            ? rootDocument.defaultView.frameElement
                            : null;
                        while (current) {
                            bounds = apply(getBounds, current, []);
                            elementScaleX =
                                current.offsetWidth > 0 ? bounds.width / current.offsetWidth : 1;
                            elementScaleY =
                                current.offsetHeight > 0 ? bounds.height / current.offsetHeight : 1;
                            x =
                                bounds.left +
                                current.clientLeft * elementScaleX +
                                x * elementScaleX;
                            y =
                                bounds.top +
                                current.clientTop * elementScaleY +
                                y * elementScaleY;
                            scaleX *= elementScaleX;
                            scaleY *= elementScaleY;
                            rootDocument = current.ownerDocument;
                            current = rootDocument.defaultView
                                ? rootDocument.defaultView.frameElement
                                : null;
                        }
                        const rootView = rootDocument.defaultView;
                        return {
                            x,
                            y,
                            scaleX,
                            scaleY,
                            scrollX: rootView ? rootView.scrollX : 0,
                            scrollY: rootView ? rootView.scrollY : 0
                        };
                    }"#,
                    "returnByValue": true,
                    "awaitPromise": true,
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
        let value = call_result_value(response?, "locate visual fallback frame owner")?;
        serde_json::from_value(value).map_err(|error| {
            PageKnotError::new(
                "pageknot.visual_fallback.protocol_shape",
                ErrorStage::Collection,
                format!("visual fallback frame owner response is invalid: {error}"),
            )
        })
    }

    async fn root_isolated_world(&self, session_id: &str) -> Result<Value> {
        let tree = self
            .client
            .command("Page.getFrameTree", json!({}), Some(session_id))
            .await?;
        let frame_id = tree
            .pointer("/frameTree/frame/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.visual_fallback.frame",
                    ErrorStage::Collection,
                    "visual fallback target has no root frame",
                )
            })?;
        let world = self
            .client
            .command(
                "Page.createIsolatedWorld",
                json!({
                    "frameId": frame_id,
                    "worldName": "pageknot-visual-fallback",
                }),
                Some(session_id),
            )
            .await?;
        world.get("executionContextId").cloned().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.visual_fallback.frame",
                ErrorStage::Collection,
                "visual fallback target world has no execution context",
            )
        })
    }
}

fn call_result_value(response: Value, operation: &str) -> Result<Value> {
    if let Some(exception) = response.get("exceptionDetails") {
        let message = exception
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("browser evaluation failed");
        return Err(PageKnotError::new(
            "pageknot.visual_fallback.evaluate",
            ErrorStage::Collection,
            format!("{operation}: {message}"),
        ));
    }
    response.pointer("/result/value").cloned().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.visual_fallback.protocol_shape",
            ErrorStage::Collection,
            format!("{operation}: browser returned no value"),
        )
    })
}

fn fallback_coordinate(value: &str, name: &str) -> Result<f64> {
    let value = value.parse::<f64>().map_err(|error| {
        PageKnotError::new(
            "pageknot.visual_fallback.bounds",
            ErrorStage::Collection,
            format!("visual fallback {name} coordinate is invalid: {error}"),
        )
    })?;
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(PageKnotError::new(
            "pageknot.visual_fallback.bounds",
            ErrorStage::Collection,
            format!("visual fallback {name} coordinate must be finite and non-negative"),
        ))
    }
}

fn fallback_dimension(value: &str, name: &str) -> Result<f64> {
    let value = value.parse::<f64>().map_err(|error| {
        PageKnotError::new(
            "pageknot.visual_fallback.bounds",
            ErrorStage::Collection,
            format!("visual fallback {name} is invalid: {error}"),
        )
    })?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(PageKnotError::new(
            "pageknot.visual_fallback.bounds",
            ErrorStage::Collection,
            format!("visual fallback {name} must be finite and positive"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{fallback_coordinate, fallback_dimension};

    #[test]
    fn fallback_bounds_are_finite_and_positive() {
        assert_eq!(fallback_coordinate("12.5", "x"), Ok(12.5));
        assert_eq!(fallback_dimension("8", "width"), Ok(8.0));
        assert!(fallback_coordinate("-1", "x").is_err());
        assert!(fallback_dimension("0", "width").is_err());
        assert!(fallback_dimension("NaN", "width").is_err());
    }
}
