//! Vulkan: device/queue setup, the glyph-atlas upload, swapchain pipeline creation, and
//! per-frame drawing (`App::redraw`, `App::dump_frame`).

use std::sync::Arc;

use vulkano::{
    DeviceSize, Validated, Version, VulkanError, VulkanLibrary,
    buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer},
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferToImageInfo, CopyImageToBufferInfo,
        RenderingAttachmentInfo, RenderingInfo, allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorImageInfo, DescriptorSet, WriteDescriptorSet,
        allocator::StandardDescriptorSetAllocator,
        layout::{
            DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo,
            DescriptorType,
        },
    },
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, DeviceFeatures, Queue, QueueCreateInfo,
        QueueFlags, physical::PhysicalDeviceType,
    },
    format::Format,
    image::{
        Image, ImageCreateInfo, ImageType, ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo},
        view::ImageView,
    },
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, PipelineBindPoint, PipelineLayout,
        PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{AttachmentBlend, ColorBlendAttachmentState, ColorBlendState},
            input_assembly::InputAssemblyState,
            multisample::MultisampleState,
            rasterization::RasterizationState,
            subpass::PipelineRenderingCreateInfo,
            vertex_input::{Vertex, VertexDefinition},
            viewport::{Viewport, ViewportState},
        },
        layout::{PipelineLayoutCreateInfo, push_constant_ranges_from_stages},
    },
    render_pass::{AttachmentLoadOp, AttachmentStoreOp},
    shader::ShaderStages,
    swapchain::{
        Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo, acquire_next_image,
    },
    sync::{self, GpuFuture},
};
use winit::{
    dpi::LogicalSize,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{CursorIcon, Window},
};

use crate::{
    app::App,
    atlas,
    shaders::{ui_fs, ui_vs},
    ui::{Ui, UiVertex, screen::draw_ui, theme::COL_BG},
};

const MAX_VERTICES: usize = 1 << 16;

#[derive(BufferContents, Clone, Copy)]
#[repr(C)]
struct Push {
    screen: [f32; 4],
}

/// Vulkan objects created once, independent of any window or swapchain.
pub(crate) struct GpuContext {
    pub(crate) instance: Arc<Instance>,
    pub(crate) device: Arc<Device>,
    pub(crate) queue: Arc<Queue>,
    pub(crate) memory_allocator: Arc<StandardMemoryAllocator>,
    pub(crate) descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    pub(crate) command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    pub(crate) sampler: Arc<Sampler>,
}

impl GpuContext {
    pub(crate) fn new(event_loop: &EventLoop<()>) -> Self {
        let library = unsafe { VulkanLibrary::new() }.unwrap();

        let required_extensions = Surface::required_extensions(event_loop);

        let instance = Instance::new(
            &library,
            &InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                enabled_extensions: &required_extensions,
                ..Default::default()
            },
        )
        .unwrap();

        let mut device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..DeviceExtensions::empty()
        };

        let (physical_device, queue_family_index) = instance
            .enumerate_physical_devices()
            .unwrap()
            .filter(|p| {
                p.api_version() >= Version::V1_3 || p.supported_extensions().khr_dynamic_rendering
            })
            .filter(|p| p.supported_extensions().contains(&device_extensions))
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .enumerate()
                    .position(|(i, q)| {
                        q.queue_flags.intersects(QueueFlags::GRAPHICS)
                            && p.presentation_support(i as u32, event_loop)
                    })
                    .map(|i| (p, i as u32))
            })
            .min_by_key(|(p, _)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                PhysicalDeviceType::Other => 4,
                _ => 5,
            })
            .expect("no suitable physical device found");

        println!(
            "Using device: {} (type: {:?})",
            physical_device.properties().device_name,
            physical_device.properties().device_type,
        );

        if physical_device.api_version() < Version::V1_3 {
            device_extensions.khr_dynamic_rendering = true;
        }

        let (device, mut queues) = Device::new(
            &physical_device,
            &DeviceCreateInfo {
                queue_create_infos: &[QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                enabled_extensions: &device_extensions,
                enabled_features: &DeviceFeatures {
                    dynamic_rendering: true,
                    ..DeviceFeatures::empty()
                },
                ..Default::default()
            },
        )
        .unwrap();

        let queue = queues.next().unwrap();

        let memory_allocator = Arc::new(StandardMemoryAllocator::new(&device, &Default::default()));
        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            &device,
            &Default::default(),
        ));
        let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
            &device,
            &Default::default(),
        ));

        // Linear filtering lets anti-aliased glyph coverage interpolate smoothly.
        let sampler = Sampler::new(
            &device,
            &SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .unwrap();

        Self {
            instance,
            device,
            queue,
            memory_allocator,
            descriptor_set_allocator,
            command_buffer_allocator,
            sampler,
        }
    }
}

/// Creates the atlas texture at its current size, uploads its pixels, and returns the
/// view plus the generation and size uploaded. `after` orders the upload behind an
/// in-flight frame still reading the previous texture.
fn upload_atlas(
    gpu: &GpuContext,
    after: Option<Box<dyn GpuFuture>>,
) -> (Arc<ImageView>, u64, [u32; 2]) {
    let (width, height) = atlas::dimensions();
    let image = Image::new(
        &gpu.memory_allocator,
        &ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: Format::R8_UNORM,
            extent: [width, height, 1],
            usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
            ..Default::default()
        },
        &AllocationCreateInfo::default(),
    )
    .unwrap();

    let staging = Buffer::from_iter(
        &gpu.memory_allocator,
        &BufferCreateInfo {
            usage: BufferUsage::TRANSFER_SRC,
            ..Default::default()
        },
        &AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        atlas::pixels(),
    )
    .unwrap();

    let mut uploads = AutoCommandBufferBuilder::primary(
        gpu.command_buffer_allocator.clone(),
        gpu.queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap();
    uploads
        .copy_buffer_to_image(CopyBufferToImageInfo::new(staging, image.clone()))
        .unwrap();
    let upload = uploads.build().unwrap();
    let start = sync::now(gpu.device.clone());
    let future = match after {
        Some(prev) => start.join(prev).boxed(),
        None => start.boxed(),
    };
    future
        .then_execute(gpu.queue.clone(), upload)
        .unwrap()
        .then_signal_fence_and_flush()
        .map_err(Validated::unwrap)
        .unwrap()
        .wait(None)
        .map_err(Validated::unwrap)
        .unwrap();

    (
        ImageView::new_default(&image).unwrap(),
        atlas::generation(),
        [width, height],
    )
}

/// Window-bound Vulkan objects, rebuilt whenever the swapchain must be recreated.
pub(crate) struct RenderContext {
    pub(crate) window: Arc<Window>,
    pub(crate) swapchain: Arc<Swapchain>,
    pub(crate) attachment_image_views: Vec<Arc<ImageView>>,
    pub(crate) pipeline: Arc<GraphicsPipeline>,
    pub(crate) descriptor_set: Arc<DescriptorSet>,
    /// The glyph atlas texture. On-demand glyphs (Hangul) bump the atlas generation,
    /// and the texture is recreated and re-uploaded behind the previous frame.
    pub(crate) atlas_view: Arc<ImageView>,
    pub(crate) atlas_gen: u64,
    pub(crate) atlas_size: [u32; 2],
    pub(crate) viewport: Viewport,
    // One vertex buffer per swapchain image. The image we just acquired is guaranteed to no
    // longer be in use by the GPU, so its buffer can always be written without a conflict.
    pub(crate) vertex_buffers: Vec<Subbuffer<[UiVertex]>>,
    pub(crate) recreate_swapchain: bool,
    pub(crate) previous_frame_end: Option<Box<dyn GpuFuture>>,
}

impl RenderContext {
    pub(crate) fn new(gpu: &GpuContext, event_loop: &ActiveEventLoop) -> Self {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Vulkan ToDo")
                        .with_inner_size(LogicalSize::new(940.0, 640.0))
                        .with_min_inner_size(LogicalSize::new(560.0, 420.0)),
                )
                .unwrap(),
        );
        let surface = Surface::from_window(&gpu.instance, &window).unwrap();
        let window_size = window.inner_size();

        let (swapchain, images) = {
            let surface_capabilities = gpu
                .device
                .physical_device()
                .surface_capabilities(&surface, &Default::default())
                .unwrap();

            let (image_format, _) = gpu
                .device
                .physical_device()
                .surface_formats(&surface, &Default::default())
                .unwrap()[0];

            Swapchain::new(
                &gpu.device,
                &surface,
                &SwapchainCreateInfo {
                    min_image_count: surface_capabilities.min_image_count.max(2),
                    image_format,
                    image_extent: window_size.into(),
                    image_usage: ImageUsage::COLOR_ATTACHMENT,
                    composite_alpha: surface_capabilities
                        .supported_composite_alpha
                        .into_iter()
                        .next()
                        .unwrap(),
                    ..Default::default()
                },
            )
            .unwrap()
        };

        let attachment_image_views = images
            .iter()
            .map(|image| ImageView::new_default(image).unwrap())
            .collect::<Vec<_>>();

        let (atlas_view, atlas_gen, atlas_size) = upload_atlas(gpu, None);

        let (pipeline, descriptor_set) = {
            let vs = unsafe { ui_vs::load(&gpu.device) }
                .unwrap()
                .entry_point("main")
                .unwrap();
            let fs = unsafe { ui_fs::load(&gpu.device) }
                .unwrap()
                .entry_point("main")
                .unwrap();

            let vertex_input_state = UiVertex::per_vertex().definition(&vs).unwrap();

            let stages = [
                PipelineShaderStageCreateInfo::new(&vs),
                PipelineShaderStageCreateInfo::new(&fs),
            ];

            let set_layout = DescriptorSetLayout::new(
                &gpu.device,
                &DescriptorSetLayoutCreateInfo {
                    bindings: &[
                        DescriptorSetLayoutBinding {
                            binding: 0,
                            descriptor_count: 1,
                            stages: ShaderStages::FRAGMENT,
                            immutable_samplers: &[&gpu.sampler],
                            ..DescriptorSetLayoutBinding::new(DescriptorType::Sampler)
                        },
                        DescriptorSetLayoutBinding {
                            binding: 1,
                            descriptor_count: 1,
                            stages: ShaderStages::FRAGMENT,
                            ..DescriptorSetLayoutBinding::new(DescriptorType::SampledImage)
                        },
                    ],
                    ..Default::default()
                },
            )
            .unwrap();

            let layout = PipelineLayout::new(
                &gpu.device,
                &PipelineLayoutCreateInfo {
                    set_layouts: &[&set_layout],
                    push_constant_ranges: &push_constant_ranges_from_stages(&stages),
                    ..Default::default()
                },
            )
            .unwrap();

            let subpass = PipelineRenderingCreateInfo {
                color_attachment_formats: &[Some(swapchain.image_format())],
                ..Default::default()
            };

            let pipeline = GraphicsPipeline::new(
                &gpu.device,
                None,
                &GraphicsPipelineCreateInfo {
                    stages: &stages,
                    vertex_input_state: Some(&vertex_input_state),
                    input_assembly_state: Some(&InputAssemblyState::default()),
                    viewport_state: Some(&ViewportState::default()),
                    rasterization_state: Some(&RasterizationState::default()),
                    multisample_state: Some(&MultisampleState::default()),
                    color_blend_state: Some(&ColorBlendState {
                        attachments: &[ColorBlendAttachmentState {
                            blend: Some(AttachmentBlend::alpha()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    dynamic_state: &[DynamicState::Viewport],
                    subpass: Some((&subpass).into()),
                    ..GraphicsPipelineCreateInfo::new(&layout)
                },
            )
            .unwrap();

            let descriptor_set = make_descriptor_set(gpu, &pipeline, &atlas_view);

            (pipeline, descriptor_set)
        };

        let viewport = Viewport {
            offset: [0.0, 0.0],
            extent: window_size.into(),
            min_depth: 0.0,
            max_depth: 1.0,
        };

        let vertex_buffers =
            create_vertex_buffers(&gpu.memory_allocator, attachment_image_views.len());

        Self {
            window,
            swapchain,
            attachment_image_views,
            pipeline,
            descriptor_set,
            atlas_view,
            atlas_gen,
            atlas_size,
            viewport,
            vertex_buffers,
            recreate_swapchain: false,
            previous_frame_end: Some(sync::now(gpu.device.clone()).boxed()),
        }
    }

    /// Brings the atlas texture back in sync with the CPU-side atlas when new glyphs
    /// were rasterized, replacing texture and descriptor set behind the previous
    /// frame so no read is interrupted.
    fn refresh_atlas(&mut self, gpu: &GpuContext) {
        if self.atlas_gen == atlas::generation() {
            return;
        }
        let previous = self.previous_frame_end.take();
        let (view, uploaded_gen, size) = upload_atlas(gpu, previous);
        self.atlas_view = view;
        self.atlas_gen = uploaded_gen;
        self.atlas_size = size;
        self.descriptor_set = make_descriptor_set(gpu, &self.pipeline, &self.atlas_view);
        self.previous_frame_end = Some(sync::now(gpu.device.clone()).boxed());
    }
}

/// The sampler + atlas-image descriptor set bound by the UI pipeline.
fn make_descriptor_set(
    gpu: &GpuContext,
    pipeline: &Arc<GraphicsPipeline>,
    atlas_view: &Arc<ImageView>,
) -> Arc<DescriptorSet> {
    DescriptorSet::new(
        &gpu.descriptor_set_allocator,
        &pipeline.layout().set_layouts()[0],
        &[WriteDescriptorSet::image(
            1,
            &DescriptorImageInfo {
                image_view: Some(atlas_view),
                ..Default::default()
            },
        )],
        &[],
    )
    .unwrap()
}

fn create_vertex_buffers(
    memory_allocator: &Arc<StandardMemoryAllocator>,
    count: usize,
) -> Vec<Subbuffer<[UiVertex]>> {
    (0..count)
        .map(|_| {
            Buffer::new_slice(
                memory_allocator,
                &BufferCreateInfo {
                    usage: BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                &AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                MAX_VERTICES as DeviceSize,
            )
            .unwrap()
        })
        .collect()
}

impl App {
    pub(crate) fn redraw(&mut self) {
        let window_size = match self.rcx.as_ref() {
            Some(rcx) => rcx.window.inner_size(),
            None => return,
        };

        if window_size.width == 0 || window_size.height == 0 {
            return;
        }

        let rcx = self.rcx.as_mut().unwrap();

        rcx.previous_frame_end.as_mut().unwrap().cleanup_finished();

        if rcx.recreate_swapchain {
            let (new_swapchain, new_images) = rcx
                .swapchain
                .recreate(&SwapchainCreateInfo {
                    image_extent: window_size.into(),
                    ..rcx.swapchain.create_info()
                })
                .expect("failed to recreate swapchain");

            rcx.swapchain = new_swapchain;
            rcx.attachment_image_views = new_images
                .iter()
                .map(|image| ImageView::new_default(image).unwrap())
                .collect::<Vec<_>>();
            rcx.vertex_buffers =
                create_vertex_buffers(&self.gpu.memory_allocator, rcx.attachment_image_views.len());
            rcx.viewport.extent = window_size.into();
            rcx.recreate_swapchain = false;
        }

        let (w, h) = (window_size.width as f32, window_size.height as f32);

        let mut ui = Ui::new(self.mouse);
        ui.clicks = std::mem::take(&mut self.pending_clicks);
        ui.mouse_down = self.mouse_down;
        ui.press = self.press;
        draw_ui(
            &mut self.todos,
            &mut self.settings,
            &self.save_path,
            &self.settings_path,
            &mut ui,
            w,
            h,
        );
        // Glyphs rasterized on demand this frame (e.g. new Hangul) go up before drawing.
        rcx.refresh_atlas(&self.gpu);
        if ui.verts.len() > MAX_VERTICES {
            ui.verts.truncate(MAX_VERTICES);
        }
        let vertex_count = ui.verts.len() as u32;

        let want_pointer = ui.pointer;
        if want_pointer != self.cursor_is_pointer {
            self.cursor_is_pointer = want_pointer;
            rcx.window.set_cursor(if want_pointer {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
        }

        let (image_index, suboptimal, acquire_future) =
            match acquire_next_image(rcx.swapchain.clone(), None).map_err(Validated::unwrap) {
                Ok(r) => r,
                Err(VulkanError::OutOfDate) => {
                    rcx.recreate_swapchain = true;
                    rcx.previous_frame_end = Some(sync::now(self.gpu.device.clone()).boxed());
                    return;
                }
                Err(e) => panic!("failed to acquire next image: {e}"),
            };

        if suboptimal {
            rcx.recreate_swapchain = true;
        }

        // The image we just acquired cannot be in use by the GPU anymore, so the vertex buffer
        // belonging to it is safe to overwrite. Writing here (instead of before acquiring) is
        // what prevents `AccessConflict(DeviceRead)` when frames are still in flight.
        let vertex_buffer = rcx.vertex_buffers[image_index as usize].clone();
        {
            let mut guard = vertex_buffer.write().unwrap();
            guard[..ui.verts.len()].copy_from_slice(&ui.verts);
        }

        let mut builder = AutoCommandBufferBuilder::primary(
            self.gpu.command_buffer_allocator.clone(),
            self.gpu.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        builder
            .begin_rendering(RenderingInfo {
                color_attachments: vec![Some(RenderingAttachmentInfo {
                    load_op: AttachmentLoadOp::Clear,
                    store_op: AttachmentStoreOp::Store,
                    clear_value: Some(COL_BG.into()),
                    ..RenderingAttachmentInfo::new(
                        rcx.attachment_image_views[image_index as usize].clone(),
                    )
                })],
                ..Default::default()
            })
            .unwrap()
            .set_viewport(0, [rcx.viewport.clone()].into_iter().collect())
            .unwrap()
            .bind_pipeline_graphics(rcx.pipeline.clone())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                rcx.pipeline.layout().clone(),
                0,
                rcx.descriptor_set.clone(),
            )
            .unwrap()
            .push_constants(
                rcx.pipeline.layout().clone(),
                0,
                Push {
                    screen: [w, h, 0.0, 0.0],
                },
            )
            .unwrap()
            .bind_vertex_buffers(0, vertex_buffer.clone())
            .unwrap();

        unsafe { builder.draw(vertex_count, 1, 0, 0) }.unwrap();

        builder.end_rendering().unwrap();

        let command_buffer = builder.build().unwrap();

        let future = sync::now(self.gpu.device.clone())
            .join(acquire_future)
            .then_execute(self.gpu.queue.clone(), command_buffer)
            .unwrap()
            .then_swapchain_present(
                self.gpu.queue.clone(),
                SwapchainPresentInfo::new(rcx.swapchain.clone(), image_index),
            )
            .then_signal_fence_and_flush();

        match future.map_err(Validated::unwrap) {
            Ok(future) => {
                rcx.previous_frame_end = Some(future.boxed());
            }
            Err(VulkanError::OutOfDate) => {
                rcx.recreate_swapchain = true;
                rcx.previous_frame_end = Some(sync::now(self.gpu.device.clone()).boxed());
            }
            Err(e) => {
                println!("failed to flush future: {e}");
                rcx.previous_frame_end = Some(sync::now(self.gpu.device.clone()).boxed());
            }
        }
    }

    /// Renders one off-screen frame and writes it as a PPM image (TODO_DUMP_FRAME env var).
    pub(crate) fn dump_frame(&mut self, path: &str) {
        let width = 940u32;
        let height = 640u32;

        let rcx = match self.rcx.as_mut() {
            Some(rcx) => rcx,
            None => return,
        };
        let color_format = rcx.swapchain.image_format();

        let mut ui = Ui::new([-1000.0; 2]);
        draw_ui(
            &mut self.todos,
            &mut self.settings,
            &self.save_path,
            &self.settings_path,
            &mut ui,
            width as f32,
            height as f32,
        );
        let vertices = ui.verts;
        let vertex_count = vertices.len() as u32;

        // Capture the pipeline and descriptor set only after draw_ui, which may have
        // rasterized new glyphs and replaced the atlas.
        rcx.refresh_atlas(&self.gpu);
        let pipeline = rcx.pipeline.clone();
        let descriptor_set = rcx.descriptor_set.clone();
        let layout = pipeline.layout().clone();

        let vertex_buffer = Buffer::from_iter(
            &self.gpu.memory_allocator,
            &BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            &AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            vertices,
        )
        .unwrap();

        let target = Image::new(
            &self.gpu.memory_allocator,
            &ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: color_format,
                extent: [width, height, 1],
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                ..Default::default()
            },
            &AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .unwrap();
        let view = ImageView::new_default(&target).unwrap();

        let readback: Subbuffer<[u8]> = Buffer::from_iter(
            &self.gpu.memory_allocator,
            &BufferCreateInfo {
                usage: BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            &AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                ..Default::default()
            },
            std::iter::repeat_n(0u8, (width * height * 4) as usize),
        )
        .unwrap();

        if let Some(rcx) = self.rcx.as_mut()
            && let Some(previous) = rcx.previous_frame_end.take()
        {
            drop(previous);
        }

        let mut builder = AutoCommandBufferBuilder::primary(
            self.gpu.command_buffer_allocator.clone(),
            self.gpu.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        builder
            .begin_rendering(RenderingInfo {
                color_attachments: vec![Some(RenderingAttachmentInfo {
                    load_op: AttachmentLoadOp::Clear,
                    store_op: AttachmentStoreOp::Store,
                    clear_value: Some(COL_BG.into()),
                    ..RenderingAttachmentInfo::new(view)
                })],
                ..Default::default()
            })
            .unwrap()
            .set_viewport(
                0,
                [Viewport {
                    offset: [0.0, 0.0],
                    extent: [width as f32, height as f32],
                    min_depth: 0.0,
                    max_depth: 1.0,
                }]
                .into_iter()
                .collect(),
            )
            .unwrap()
            .bind_pipeline_graphics(pipeline)
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                layout.clone(),
                0,
                descriptor_set,
            )
            .unwrap()
            .push_constants(
                layout,
                0,
                Push {
                    screen: [width as f32, height as f32, 0.0, 0.0],
                },
            )
            .unwrap()
            .bind_vertex_buffers(0, vertex_buffer)
            .unwrap();

        unsafe { builder.draw(vertex_count, 1, 0, 0) }.unwrap();

        builder.end_rendering().unwrap();
        builder
            .copy_image_to_buffer(CopyImageToBufferInfo::new(target, readback.clone()))
            .unwrap();

        let command_buffer = builder.build().unwrap();
        sync::now(self.gpu.device.clone())
            .then_execute(self.gpu.queue.clone(), command_buffer)
            .unwrap()
            .then_signal_fence_and_flush()
            .map_err(Validated::unwrap)
            .unwrap()
            .wait(None)
            .map_err(Validated::unwrap)
            .unwrap();

        let data = readback.read().unwrap();
        let bgra = matches!(
            color_format,
            Format::B8G8R8A8_UNORM | Format::B8G8R8A8_SRGB | Format::B8G8R8A8_SNORM
        );
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        for px in data.as_chunks::<4>().0 {
            if bgra {
                ppm.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                ppm.extend_from_slice(&[px[0], px[1], px[2]]);
            }
        }
        std::fs::write(path, ppm).unwrap();
        println!("debug frame written to {path}");
    }
}
