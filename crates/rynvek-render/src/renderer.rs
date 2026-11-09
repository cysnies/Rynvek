// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WebGPU 世界渲染器。
//!
//! 一帧只有一个渲染通道、两个颜色附件：`location(0)` 颜色（画布 / 离屏纹理）与
//! `location(1)` R32Uint 拾取目标。绘制顺序严格为 L0→L7：
//! 1. 全屏三角形绘制 L0 地形与 L1 场热力图（场数据直接绑定 GPU 驻留缓冲）；
//! 2. 按层依次实例化绘制 L2–L7；可拾取层写入拾取编号，不可拾取层（L6 迷雾）的拾取写掩码为空。
//!
//! BindGroup 约定与计算内核一致：`@group(0)` Frame（Uniform），`@group(1)` StateIn（只读 GPU 状态）。

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::camera::Camera2D;
use crate::instance::InstanceData;
use crate::layer::RenderLayer;
use crate::palette::WorldPalette;
use crate::scene::FrameScene;

/// 拾取目标格式
pub const PICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// L1 热力图的数据来源：GPU 驻留的含 Halo 场缓冲（`stride² × i32`）
#[derive(Clone, Copy, Debug)]
pub struct FieldOverlay<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub tile_dim: u32,
    pub halo: u32,
    /// 场原点的世界坐标（格）
    pub origin: [f32; 2],
    /// 每个场格覆盖的世界格数
    pub cell_size: f32,
    /// 映射到热力图顶端的 Q8.8 原始值
    pub value_max: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct OverlayUniform {
    tile_dim: u32,
    halo: u32,
    stride: u32,
    flags: u32,
    origin: [f32; 2],
    cell_size: f32,
    value_max: f32,
    background: u32,
    grid_line: u32,
    heat_low: u32,
    heat_high: u32,
}

const FLAG_TERRAIN: u32 = 1;
const FLAG_GRID: u32 = 2;
const FLAG_HEATMAP: u32 = 4;

/// 离屏渲染目标（测试、截图视觉回归与无 Surface 的原生宿主）
#[derive(Debug)]
pub struct OffscreenTarget {
    pub color: wgpu::Texture,
    pub pick: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub pick_view: wgpu::TextureView,
    pub size: [u32; 2],
}

impl OffscreenTarget {
    pub fn new(device: &wgpu::Device, size: [u32; 2], color_format: wgpu::TextureFormat) -> Self {
        let color = create_target(device, "rynvek.render.color", size, color_format);
        let pick = create_target(device, "rynvek.render.pick", size, PICK_FORMAT);
        Self {
            color_view: color.create_view(&Default::default()),
            pick_view: pick.create_view(&Default::default()),
            color,
            pick,
            size,
        }
    }

    /// 阻塞回读颜色纹理为紧密排列的 RGBA8（仅测试 / 截图）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_color_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        read_texture_blocking(device, queue, &self.color, 4)
    }

    /// 阻塞回读拾取纹理（仅测试）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_pick_blocking(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32> {
        bytemuck::pod_collect_to_vec(&read_texture_blocking(device, queue, &self.pick, 4))
    }
}

/// 创建与画布同尺寸的拾取纹理（浏览器 Surface 宿主使用）
pub fn create_pick_texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    create_target(device, "rynvek.render.pick", size, PICK_FORMAT)
}

fn create_target(device: &wgpu::Device, label: &str, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn read_texture_blocking(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, bpp: u32) -> Vec<u8> {
    let size = texture.size();
    let row = size.width * bpp;
    let padded_row = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rynvek.render.readback"),
        size: u64::from(padded_row * size.height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    queue.submit([enc.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let mut out = Vec::with_capacity((row * size.height) as usize);
    if let Ok(view) = buffer.slice(..).get_mapped_range() {
        for y in 0..size.height {
            let start = (y * padded_row) as usize;
            out.extend_from_slice(&view[start..start + row as usize]);
        }
    }
    buffer.unmap();
    out
}

/// 世界渲染器
#[derive(Debug)]
pub struct WorldRenderer {
    color_format: wgpu::TextureFormat,
    camera_buffer: wgpu::Buffer,
    overlay_buffer: wgpu::Buffer,
    dummy_field: wgpu::Buffer,
    fullscreen_frame_layout: wgpu::BindGroupLayout,
    fullscreen_field_layout: wgpu::BindGroupLayout,
    fullscreen_pipeline: wgpu::RenderPipeline,
    instance_frame: wgpu::BindGroup,
    instance_pickable: wgpu::RenderPipeline,
    instance_visual: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    /// 是否绘制网格线
    pub show_grid: bool,
    pub palette: WorldPalette,
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl WorldRenderer {
    pub const FULLSCREEN_WGSL: &'static str = include_str!("../shaders/world_fullscreen.wgsl");
    pub const INSTANCES_WGSL: &'static str = include_str!("../shaders/world_instances.wgsl");

    /// 创建渲染器；`color_format` 为画布 Surface 或离屏纹理格式（推荐非 sRGB 的 `Rgba8Unorm` / `Bgra8Unorm`）
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> rynvek_wgpu::GpuResult<Self> {
        let fs_module = rynvek_wgpu::ShaderLibrary::create_module_from_wgsl(
            device,
            "rynvek.render.world_fullscreen",
            Self::FULLSCREEN_WGSL,
        )?;
        let inst_module = rynvek_wgpu::ShaderLibrary::create_module_from_wgsl(
            device,
            "rynvek.render.world_instances",
            Self::INSTANCES_WGSL,
        )?;
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rynvek.render.camera"),
            size: std::mem::size_of::<crate::camera::CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rynvek.render.overlay"),
            size: std::mem::size_of::<OverlayUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_field = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rynvek.render.field.dummy"),
            contents: &[0u8; 4],
            usage: wgpu::BufferUsages::STORAGE,
        });

        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let fullscreen_frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rynvek.render.bind_group.0.frame"),
            entries: &[uniform_entry(0, vf), uniform_entry(1, vf)],
        });
        let fullscreen_field_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rynvek.render.bind_group.1.state_in"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let instance_frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rynvek.render.bind_group.0.frame.instances"),
            entries: &[uniform_entry(0, vf)],
        });
        let instance_frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rynvek.render.bind_group.0.frame.instances"),
            layout: &instance_frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let color_target = |write_pick: bool| {
            [
                Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: PICK_FORMAT,
                    blend: None,
                    write_mask: if write_pick {
                        wgpu::ColorWrites::ALL
                    } else {
                        wgpu::ColorWrites::empty()
                    },
                }),
            ]
        };

        let fs_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rynvek.render.fullscreen"),
            bind_group_layouts: &[Some(&fullscreen_frame_layout), Some(&fullscreen_field_layout)],
            immediate_size: 0,
        });
        let fs_targets = color_target(false);
        let fullscreen_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rynvek.render.L0_L1.fullscreen"),
            layout: Some(&fs_layout),
            vertex: wgpu::VertexState {
                module: &fs_module,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs_module,
                entry_point: Some("fs_world"),
                compilation_options: Default::default(),
                targets: &fs_targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let inst_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rynvek.render.instances"),
            bind_group_layouts: &[Some(&instance_frame_layout)],
            immediate_size: 0,
        });
        let make_instance_pipeline = |label: &str, write_pick: bool| {
            let targets = color_target(write_pick);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&inst_layout),
                vertex: wgpu::VertexState {
                    module: &inst_module,
                    entry_point: Some("vs_instance"),
                    compilation_options: Default::default(),
                    buffers: &[Some(InstanceData::vertex_layout())],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &inst_module,
                    entry_point: Some("fs_instance"),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let instance_pickable = make_instance_pipeline("rynvek.render.instances.pickable", true);
        let instance_visual = make_instance_pipeline("rynvek.render.instances.visual", false);
        let instance_capacity = 1024;
        let instance_buffer = Self::create_instance_buffer(device, instance_capacity);

        Ok(Self {
            color_format,
            camera_buffer,
            overlay_buffer,
            dummy_field,
            fullscreen_frame_layout,
            fullscreen_field_layout,
            fullscreen_pipeline,
            instance_frame,
            instance_pickable,
            instance_visual,
            instance_buffer,
            instance_capacity,
            show_grid: true,
            palette: WorldPalette::NEUTRAL_DARK,
        })
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rynvek.render.instances"),
            size: capacity * std::mem::size_of::<InstanceData>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.color_format
    }

    /// 录制一帧：`color_view` 与 `pick_view` 尺寸必须等于 `camera.viewport`
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        pick_view: &wgpu::TextureView,
        scene: &FrameScene,
        camera: &Camera2D,
        field: Option<FieldOverlay<'_>>,
    ) {
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera.uniform()));
        let mut flags = 0;
        if scene.layers.contains(RenderLayer::Terrain) {
            flags |= FLAG_TERRAIN;
            if self.show_grid {
                flags |= FLAG_GRID;
            }
        }
        let heat = field.filter(|_| scene.layers.contains(RenderLayer::FieldHeatmap));
        if heat.is_some() {
            flags |= FLAG_HEATMAP;
        }
        let p = &self.palette;
        let overlay = OverlayUniform {
            tile_dim: heat.map_or(0, |f| f.tile_dim),
            halo: heat.map_or(0, |f| f.halo),
            stride: heat.map_or(0, |f| f.tile_dim + 2 * f.halo),
            flags,
            origin: heat.map_or([0.0; 2], |f| f.origin),
            cell_size: heat.map_or(1.0, |f| f.cell_size.max(1e-6)),
            value_max: heat.map_or(1.0, |f| f.value_max.max(1.0)),
            background: p.background.packed(),
            grid_line: p.grid_line.packed(),
            heat_low: p.heat_low.packed(),
            heat_high: p.heat_high.packed(),
        };
        queue.write_buffer(&self.overlay_buffer, 0, bytemuck::bytes_of(&overlay));

        // 场缓冲随 CFL 子步乒乓切换，因此每帧按当前缓冲重建 BindGroup（开销极小）
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rynvek.render.bind_group.0.frame"),
            layout: &self.fullscreen_frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.overlay_buffer.as_entire_binding(),
                },
            ],
        });
        let field_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rynvek.render.bind_group.1.state_in"),
            layout: &self.fullscreen_field_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: heat.map_or(&self.dummy_field, |f| f.buffer).as_entire_binding(),
            }],
        });

        let instances = scene.ordered_instances();
        if instances.len() as u64 > self.instance_capacity {
            self.instance_capacity = (instances.len() as u64).next_power_of_two();
            self.instance_buffer = Self::create_instance_buffer(device, self.instance_capacity);
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }

        let bg = p.background.to_f64();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rynvek.render.world"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0],
                            g: bg[1],
                            b: bg[2],
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: pick_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            ..Default::default()
        });

        if flags != 0 {
            pass.set_pipeline(&self.fullscreen_pipeline);
            pass.set_bind_group(0, &frame_group, &[]);
            pass.set_bind_group(1, &field_group, &[]);
            pass.draw(0..3, 0..1);
        }

        if !instances.is_empty() {
            pass.set_bind_group(0, &self.instance_frame, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            let mut start = 0u32;
            for layer in RenderLayer::ALL {
                let n = scene.layer_instances(layer).len() as u32;
                if n == 0 {
                    continue;
                }
                pass.set_pipeline(if layer.is_pickable() {
                    &self.instance_pickable
                } else {
                    &self.instance_visual
                });
                pass.draw(0..6, start..start + n);
                start += n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_shaders_pass_naga_validation() {
        for (label, src) in [
            ("world_fullscreen", WorldRenderer::FULLSCREEN_WGSL),
            ("world_instances", WorldRenderer::INSTANCES_WGSL),
        ] {
            rynvek_wgpu::validate_wgsl(label, src).unwrap();
        }
        assert_eq!(std::mem::size_of::<OverlayUniform>(), 48);
    }
}
