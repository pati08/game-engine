use std::io::{BufReader, Cursor};

use super::model::Model;
use wgpu::util::DeviceExt;

use super::{model, texture};

pub async fn load_string(file_name: &str) -> anyhow::Result<String> {
    let txt = {
        let path = std::path::Path::new("./rsc").join(file_name);
        std::fs::read_to_string(path)?
    };

    Ok(txt)
}

pub async fn load_binary(file_name: &str) -> anyhow::Result<Vec<u8>> {
    let data = {
        let path = std::path::Path::new("./rsc").join(file_name);
        std::fs::read(path)?
    };

    Ok(data)
}

pub async fn load_texture(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<texture::Texture, anyhow::Error> {
    let data = load_binary(file_name).await?;
    texture::Texture::from_bytes(device, queue, &data, file_name)
}

pub async fn load_model(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<Model> {
    let obj_text = load_string(file_name).await?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let (models, obj_materials) = tobj::load_obj_buf_async(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| async move {
            let mat_text = load_string(&p).await.unwrap();
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        },
    )
    .await?;

    let mut materials = Vec::new();
    for m in obj_materials? {
        let diffuse_texture = load_texture(
            &m.diffuse_texture.expect("material lacks diffuse texture"),
            device,
            queue,
        )
        .await?;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: None,
        });
        materials.push(model::Material {
            name: m.name,
            diffuse_texture,
            bind_group,
        });
    }

    let meshes = models
        .into_iter()
        .map(|mesh| {
            let vertices = (0..mesh.mesh.positions.len() / 3)
                .map(|i| {
                    let norm = if mesh.mesh.normals.is_empty() {
                        [0.0, 0.0, 0.0]
                    } else {
                        [
                            mesh.mesh.normals[i * 3],
                            mesh.mesh.normals[i * 3 + 1],
                            mesh.mesh.normals[i * 3 + 2],
                        ]
                    };
                    model::ModelVertex {
                        position: [
                            mesh.mesh.positions[i * 3],
                            mesh.mesh.positions[i * 3 + 1],
                            mesh.mesh.positions[i * 3 + 2],
                        ],
                        tex_coords: [mesh.mesh.texcoords[i * 2], mesh.mesh.texcoords[i * 2 + 1]],
                        normal: norm,
                    }
                })
                .collect::<Vec<_>>();
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{file_name:?} Vertex Buffer")),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{file_name:?} Index Buffer")),
                contents: bytemuck::cast_slice(&mesh.mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            model::Mesh {
                name: file_name.to_string(),
                vertex_buffer,
                index_buffer,
                num_elements: mesh.mesh.indices.len() as u32,
                material: mesh.mesh.material_id.unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    Ok(model::Model { meshes, materials })
}
