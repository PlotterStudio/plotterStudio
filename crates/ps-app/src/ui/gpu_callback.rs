//! The canvas drawn on the GPU inside egui's own render pass: an `egui_wgpu`
//! paint callback that draws a [`Scene`] with the mesh pipeline, sharing egui's device. Overlays
//! (selection handles, tool previews) are drawn over it by egui's painter.

use egui_wgpu::{Callback, CallbackResources, CallbackTrait, ScreenDescriptor};
use ps_render::{GpuContext, Scene};

/// One frame's canvas scene and the pixel size it is drawn at.
struct SceneCallback {
    scene: Scene,
    format: wgpu::TextureFormat,
}

impl CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<GpuContext>().is_none() {
            resources.insert(GpuContext::from_device(
                device.clone(),
                queue.clone(),
                self.format,
            ));
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<GpuContext>() {
            let area = info.viewport_in_pixels();
            let width = u32::try_from(area.width_px).unwrap_or(0);
            let height = u32::try_from(area.height_px).unwrap_or(0);
            gpu.draw_into(pass, &self.scene, width, height);
        }
    }
}

/// An egui shape that draws `scene` into `rect`.
pub(super) fn scene_shape(
    rect: egui::Rect,
    scene: Scene,
    format: wgpu::TextureFormat,
) -> egui::Shape {
    egui::Shape::Callback(Callback::new_paint_callback(
        rect,
        SceneCallback { scene, format },
    ))
}
