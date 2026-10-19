# API Ruby de SketchUp 2026 (extraite de SketchUp.exe : appels rb_define_*)

Noms et arités seulement (interface publique). `/*` = arité variable, `self.` = méthode de classe.

## BoundingBox (22)
add/*, center/0, clear/0, CONST CORNER_LEFT_BACK_BOTTOM, CONST CORNER_LEFT_BACK_TOP, CONST CORNER_LEFT_FRONT_BOTTOM, CONST CORNER_LEFT_FRONT_TOP, CONST CORNER_RIGHT_BACK_BOTTOM, CONST CORNER_RIGHT_BACK_TOP, CONST CORNER_RIGHT_FRONT_BOTTOM, CONST CORNER_RIGHT_FRONT_TOP, contains?/1, corner/1, depth/0, diagonal/0, empty?/0, height/0, intersect/1, max/0, min/0, valid?/0, width/0

## Bounds2d (8)
==/1, height/0, initialize/*, lower_right/0, set!/*, to_a/0, upper_left/0, width/0

## Color (15)
==/1, alpha/0, alpha=/1, blend/2, blue/0, blue=/1, green/0, green=/1, initialize/*, red/0, red=/1, self.names/0, to_a/0, to_i/0, to_s/0

## ExtensionLicense (4)
days_remaining/0, error_description/0, licensed?/0, state/0

## Geom (8)
self.closest_points/2, self.fit_plane_to_points/*, self.intersect_line_line/2, self.intersect_line_plane/2, self.intersect_plane_plane/2, self.linear_combination/4, self.point_in_polygon_2D/3, self.tesselate/*

## LatLong (6)
initialize/*, latitude/0, longitude/0, to_a/0, to_s/0, to_utm/0

## Length (32)
</1, <=/1, <=>/1, ==/1, >/1, >=, CONST Architectural, CONST Centimeter, CONST CubicCentimeter, CONST CubicFeet, CONST CubicInches, CONST CubicMeter, CONST CubicMillimeter, CONST CubicYard, CONST Decimal, CONST Engineering, CONST Feet, CONST Fractional, CONST Inches, CONST Liter, CONST Meter, CONST Millimeter, CONST SquareCentimeter, CONST SquareFeet, CONST SquareInches, CONST SquareMeter, CONST SquareMillimeter, CONST SquareYard, CONST USGallon, CONST Yard, inspect/0, to_s/0

## Point2d (21)
-/1, []/1, []=/2, +/1, ==/1, clone/0, distance/1, initialize/*, inspect/0, offset!/*, offset/*, set!/*, to_a/0, to_s/0, transform!/1, transform/1, vector_to/1, x/0, x=/1, y/0, y=/1

## Point3d (31)
-/1, []/1, []=/2, +/1, </1, ==/1, clone/0, distance/1, distance_to_line/*, distance_to_plane/*, initialize/*, inspect/0, offset!/*, offset/*, on_line?/*, on_plane?/*, project_to_line/*, project_to_plane/*, self.linear_combination/4, set!/*, to_a/0, to_s/0, transform!/1, transform/1, vector_to/1, x/0, x=/1, y/0, y=/1, z/0, z=/1

## PolygonMesh (26)
add_point/*, add_polygon/*, CONST AUTO_SOFTEN, CONST HIDE_BASED_ON_INDEX, CONST MESH_NORMALS, CONST MESH_POINTS, CONST MESH_UVQ_BACK, CONST MESH_UVQ_FRONT, CONST NO_SMOOTH_OR_HIDE, CONST SMOOTH_SOFT_EDGES, CONST SOFTEN_BASED_ON_INDEX, count_points/0, count_polygons/0, initialize/*, normal_at/1, point_at/1, point_index/1, points/0, polygon_at/1, polygon_points_at/1, polygons/0, set_point/2, set_uv/3, transform!/1, uv_at/2, uvs/1

## rb_cArray (22)
cross/1, distance/1, distance_to_line/*, distance_to_plane/*, dot/1, normalize!/0, normalize/0, offset!/*, offset/*, on_line?/*, on_plane?/*, project_to_line/*, project_to_plane/*, transform!/1, transform/1, vector_to/1, x/0, x=/1, y/0, y=/1, z/0, z=/1

## rb_cNumeric (19)
cm, degrees, feet, inch, km, m, mile, mm, radians, to_cm, to_feet, to_inch, to_km, to_l, to_m, to_mile, to_mm, to_yard, yard

## rb_cString (1)
to_l

## Request (13)
body/0, body=/1, cancel/0, headers/0, headers=/1, initialize/*, method/0, method=/1, set_download_progress_callback/0, set_upload_progress_callback/0, start/0, status/0, url/0

## Response (3)
body/0, headers/0, status_code/0

## Sketchup (79)
self.active_model/0, self.add_observer/1, self.app_name/0, self.break_edges?/0, self.break_edges=/1, self.create_texture_writer/0, self.debug_mode?/0, self.debug_mode=/1, self.device_id/0, self.display_name_from_action/1, self.extensions/0, self.file_new/0, self.find_support_file/*, self.find_support_files/*, self.fix_shadow_strings?/0, self.fix_shadow_strings=/1, self.focus/0, self.format_angle/1, self.format_area/1, self.format_degrees/1, self.format_length/*, self.format_volume/1, self.full_detail_render_delay_max/0, self.full_detail_render_delay_max=/1, self.full_detail_render_delay_min/0, self.full_detail_render_delay_min=/1, self.get_datfile_info/2, self.get_i18n_datfile_info/2, self.get_locale/0, self.get_resource_path/1, self.get_shortcuts/0, self.has_feature?/1, self.install_from_archive/*, self.is_64bit?/0, self.is_online/0, self.is_pro?/0, self.is_valid_filename?/1, self.load, self.max_draw_time/0, self.max_draw_time=/1, self.mdi?/0, self.open_file/*, self.os_language/0, self.parse_length/1, self.platform/0, self.plugins_disabled?/0, self.plugins_disabled=/1, self.quit/0, self.read_default/*, self.redo/0, self.register_extension/*, self.register_importer/1, self.register_procedure/1, self.remove_observer/1, self.require, self.required_camera_lod/0, self.required_camera_lod=/1, self.required_tool_lod/0, self.required_tool_lod=/1, self.resize_viewport/*, self.save_thumbnail/2, self.send_action/1, self.send_to_layout/1, self.set_status_text/*, self.sign_in/*, self.signed_in?/0, self.status_text=/1, self.temp_dir/0, self.template/0, self.template_dir/0, self.template=/1, self.undo/0, self.use_lod?/0, self.use_lod=/1, self.vcb_label=/1, self.vcb_value=/1, self.version/0, self.version_number/*, self.write_default/3

## Sketchup::Amplitude (4)
self.close_preview/0, self.get_all_guides, self.open_preview/0, self.update_preview/1

## Sketchup::AppObserver (3)
onNewModel, onOpenModel, onQuit

## Sketchup::ArcCurve (9)
center/0, circular?/0, end_angle/0, normal/0, plane/0, radius/0, start_angle/0, xaxis/0, yaxis/0

## Sketchup::AttributeDictionaries (5)
[]/1, delete/1, each/0, length/0, size/0

## Sketchup::AttributeDictionary (12)
[]/1, []=/2, delete_key/1, each/0, each_key/0, each_pair/0, empty?/0, keys/0, length/0, name/0, size/0, values/0

## Sketchup::Axes (10)
axes/0, origin/0, set/4, sketch_plane/0, to_a/0, transformation/0, typename/0, xaxis/0, yaxis/0, zaxis/0

## Sketchup::Behavior (12)
always_face_camera?/0, always_face_camera=/1, cuts_opening?/0, cuts_opening=/1, is2d?/0, is2d=/1, no_scale_mask?/0, no_scale_mask=/1, shadows_face_sun?/0, shadows_face_sun=/1, snapto/0, snapto=/1

## Sketchup::Camera (29)
aspect_ratio/0, aspect_ratio=/1, center_2d/0, description/0, description=/1, direction/0, eye/0, focal_length/0, focal_length=/1, fov/0, fov_is_height?/0, fov=/1, height/0, height=/1, image_width/0, image_width=/1, initialize, is_2d?/0, perspective?/0, perspective=/1, scale_2d/0, self.deserialize/1, serialize/0, set/*, target/0, up/0, xaxis/0, yaxis/0, zaxis/0

## Sketchup::Classifications (7)
[]/1, each/0, keys/0, length/0, load_schema/1, size/0, unload_schema/1

## Sketchup::ClassificationSchema (3)
<=>/1, name/0, namespace/0

## Sketchup::ComponentDefinition (40)
<=>/1, ==/1, add_classification/2, add_observer/1, attach_procedure/2, behavior/0, control_entities/0, count_instances/0, count_used_instances/0, description/0, description=/1, entities/0, get_classification_value/1, group?/0, guid/0, hidden?/0, image?/0, insertion_point/0, insertion_point=/1, instances/0, internal?/0, invalidate_bounds/0, live_component?/0, load_time/0, manifold?/0, merge/*, name/0, name=/1, path/0, procedural?/0, refresh_thumbnail/0, remove_classification/*, remove_observer/1, save_as, save_copy, save_thumbnail/1, set_classification_value/2, set_procedure_parameters/1, thumbnail_camera/0, thumbnail_camera=/1

## Sketchup::ComponentInstance (29)
add_observer/1, definition/0, definition=/1, equals?/1, explode/0, glued_to/0, glued_to=/1, guid/0, hatch_pattern/0, hatch_pattern=/1, intersect/1, locked?/0, locked=/1, make_unique/0, manifold?/0, move!/1, name/0, name=/1, outer_shell/1, remove_observer/1, show_differences/*, split/1, subtract/1, transform!/1, transformation/0, transformation=/1, trim/1, union/1, volume/0

## Sketchup::Console (8)
<</1, clear/0, flush, hide/0, show/0, sync, visible?/0, write/1

## Sketchup::ConstructionLine (11)
direction/0, direction=/1, end/0, end=/1, position/0, position=/1, reverse!/0, start/0, start=/1, stipple/0, stipple=/1

## Sketchup::ConstructionPoint (1)
position/0

## Sketchup::Curve (9)
count_edges/0, each_edge/0, edges/0, first_edge/0, is_polygon?/0, last_edge/0, length/0, move_vertices/1, vertices/0

## Sketchup::DefinitionList (14)
[]/1, add/*, add_observer/1, at/1, each/0, import/*, length/0, load/*, load_from_url/*, purge_unused/0, remove/1, remove_observer/1, size/0, unique_name/1

## Sketchup::Dimension (14)
add_observer/1, arrow_type/0, arrow_type=/1, CONST ARROW_CLOSED, CONST ARROW_DOT, CONST ARROW_NONE, CONST ARROW_OPEN, CONST ARROW_SLASH, has_aligned_text?/0, has_aligned_text=/1, plane/0, remove_observer/1, text/0, text=/1

## Sketchup::DimensionLinear (20)
aligned_text_position/0, aligned_text_position=/1, CONST ALIGNED_TEXT_ABOVE, CONST ALIGNED_TEXT_CENTER, CONST ALIGNED_TEXT_OUTSIDE, CONST TEXT_CENTERED, CONST TEXT_OUTSIDE_END, CONST TEXT_OUTSIDE_START, end, end_attached_to, end_attached_to=, end=, offset_vector/0, offset_vector=/1, start, start_attached_to, start_attached_to=, start=, text_position/0, text_position=/1

## Sketchup::DimensionObserver (1)
onTextChanged

## Sketchup::DimensionRadial (5)
arc_curve/0, arc_curve=/1, leader_break_point/0, leader_break_point=/1, leader_points/0

## Sketchup::Drawingelement (14)
bounds/0, casts_shadows?/0, casts_shadows=/1, erase!/0, hidden?/0, hidden=/1, layer/0, layer=/1, material/0, material=/1, receives_shadows?/0, receives_shadows=/1, visible?/0, visible=/1

## Sketchup::Edge (19)
all_connected/0, common_face/1, curve/0, end/0, explode_curve/0, faces/0, find_faces/0, length/*, line/0, other_vertex/1, reversed_in?/1, smooth?/0, smooth=/1, soft?/0, soft=/1, split/1, start/0, used_by?/1, vertices/0

## Sketchup::EdgeUse (9)
edge/0, end_vertex_normal/0, face/0, loop/0, next/0, partners/0, previous/0, reversed?/0, start_vertex_normal/0

## Sketchup::Entities (39)
[]/1, active_section_plane/0, active_section_plane=/1, add_3d_text/*, add_arc/*, add_circle, add_cline/*, add_cpoint/1, add_curve/*, add_dimension_linear/*, add_dimension_radial/2, add_edges/*, add_face/*, add_faces_from_mesh/*, add_group/*, add_image/*, add_instance/2, add_line/*, add_ngon, add_observer/1, add_section_plane/*, add_snap/*, add_space/4, add_text/*, at/1, build/0, clear!/0, each/0, erase_entities/*, fill_from_mesh/*, intersect_with/*, length/0, model/0, parent/0, remove_observer/1, size/0, transform_by_vectors/*, transform_entities/*, weld/1

## Sketchup::EntitiesBuilder (9)
add_edge/*, add_edges/*, add_face/*, add_line, clone/0, dup/0, entities/0, valid?/0, vertex_at/1

## Sketchup::Entity (16)
add_observer/1, attribute_dictionaries/0, attribute_dictionary/*, delete_attribute/*, deleted?/0, entityID/0, get_attribute/*, inspect/0, model/0, parent/0, persistent_id/0, remove_observer/1, set_attribute/*, to_s/0, typename/0, valid?/0

## Sketchup::Environment (21)
description/0, description=/1, linked_sun?/0, linked_sun_position/0, linked_sun_position=/1, linked_sun=/1, name/0, name=/1, path/0, reflection_exposure/0, reflection_exposure=/1, rotation/0, rotation=/1, skydome_exposure/0, skydome_exposure=/1, thumbnail/0, use_as_skydome?/0, use_as_skydome=/1, use_for_reflections?/0, use_for_reflections=/1, write_hdr/1

## Sketchup::Environments (11)
[]/1, add/*, add_observer/1, current/0, current=/1, each/0, length, purge_unused/0, remove/1, remove_observer/1, size/0

## Sketchup::ExtensionsManager (5)
[]/1, each/0, keys/0, length/0, size/0

## Sketchup::Face (33)
all_connected/0, area/*, back_material/0, back_material=/1, classify_point/1, clear_texture_position/1, clear_texture_projection/1, CONST PointInside, CONST PointNotOnPlane, CONST PointOnEdge, CONST PointOnFace, CONST PointOnVertex, CONST PointOutside, CONST PointUnknown, coplanar_with?/1, edges/0, followme/*, get_glued_instances/0, get_texture_projection/1, get_UVHelper/*, loops/0, mesh/*, normal/0, outer_loop/0, plane/0, position_material/*, pushpull/*, reverse!/0, set_texture_projection/2, texture_positioned?/1, texture_projected?/1, uv_tile_at/2, vertices/0

## Sketchup::Group (34)
add_observer/1, copy/0, definition/0, description/0, description=/1, entities/0, equals?/1, explode/0, glued_to/0, glued_to=/1, guid/0, hatch_pattern/0, hatch_pattern=/1, intersect/1, local_bounds/0, locked?/0, locked=/1, make_unique/0, manifold?/0, move!/1, name/0, name=/1, outer_shell/1, remove_observer/1, show_differences/*, split/1, subtract/1, to_component/0, transform!/1, transformation/0, transformation=/1, trim/1, union/1, volume/0

## Sketchup::HatchPattern (6)
height/0, name/0, name=/1, size=/1, width/0, write/1

## Sketchup::HatchPatterns (7)
[]/1, add/2, each/0, length, purge_unused/0, remove/1, size/0

## Sketchup::HLR (4)
calculate/0, cleanup/0, num_faces/0, self.use_new_edgevisibility

## Sketchup::Http (12)
CONST DELETE, CONST GET, CONST HEAD, CONST OPTIONS, CONST PATCH, CONST POST, CONST PUT, CONST STATUS_CANCELED, CONST STATUS_FAILED, CONST STATUS_PENDING, CONST STATUS_SUCCESS, CONST STATUS_UNKNOWN

## Sketchup::Image (19)
explode/0, glued_to/0, glued_to=/1, height/0, height=/1, image_rep/0, normal/0, origin/0, origin=/1, path/0, pixelheight/0, pixelwidth/0, size=/1, transform!/1, transformation/0, transformation=/1, width/0, width=/1, zrotation/0

## Sketchup::ImageRep (12)
bits_per_pixel/0, color_at_uv/*, colors/0, data/0, height/0, initialize/*, load_file/1, row_padding/0, save_file/1, set_data/5, size/0, width/0

## Sketchup::Importer (11)
CONST ImportCanceled, CONST ImportFail, CONST ImportFileNotFound, CONST ImportSuccess, description, do_options, file_extension, id, load_file, show_summary, supports_options?

## Sketchup::InputPoint (17)
==/1, clear/0, copy!/1, degrees_of_freedom/0, depth/0, display?/0, draw/1, edge/0, face/0, initialize/*, instance_path/0, pick/*, position/0, tooltip/0, transformation/0, valid?/0, vertex/0

## Sketchup::InstancePath (20)
[]/1, ==/1, clear/0, clone/0, copy/1, each/0, empty?/0, include?/1, initialize/1, leaf/0, leaf=/1, length/0, persistent_id_path/0, pop/*, push/1, root/0, size/0, to_a/0, transformation/*, valid?/0

## Sketchup::Layer (15)
<=>/1, ==/1, color/0, color=/1, display_name/0, folder/0, folder=/1, line_style/0, line_style=/1, name/0, name=/1, page_behavior/0, page_behavior=/1, visible?/0, visible=/1

## Sketchup::LayerFolder (24)
<=>/1, ==/1, add_folder/1, add_layer/1, count_folders/0, count_layers/0, display_name, each, each_folder/0, each_layer/0, folder/0, folder=/1, folders/0, layers/0, length, name/0, name=/1, remove_folder/1, remove_layer/1, size, visible?/0, visible_on_new_pages?/0, visible_on_new_pages=/1, visible=/1

## Sketchup::Layers (24)
[]/1, add/1, add_folder/1, add_layer, add_observer/1, at/1, count_folders/0, count_layers/0, dump/0, each/0, each_folder/0, each_layer/0, folders/0, layers/0, length/0, purge_unused/0, purge_unused_folders/0, purge_unused_layers, remove/*, remove_folder/1, remove_layer, remove_observer/1, size/0, unique_name/*

## Sketchup::Licensing (6)
CONST EXPIRED, CONST LICENSED, CONST NOT_LICENSED, CONST TRIAL, CONST TRIAL_EXPIRED, self.get_extension_license/1

## Sketchup::LineStyle (1)
name/0

## Sketchup::LineStyles (7)
[]/1, at/1, each/0, length, names/0, size/0, to_a/0

## Sketchup::Loop (6)
convex?/0, edges/0, edgeuses/0, face/0, outer?/0, vertices/0

## Sketchup::Material (61)
<=>/1, ==/1, alpha/0, alpha=/1, ao_enabled?/0, ao_enabled=/1, ao_strength/0, ao_strength=/1, ao_texture/0, ao_texture=/1, color/0, color=/1, colorize_deltas/0, colorize_type/0, colorize_type=/1, CONST COLORIZE_SHIFT, CONST COLORIZE_TINT, CONST MATERIAL_COLORIZED_TEXTURED, CONST MATERIAL_SOLID, CONST MATERIAL_TEXTURED, CONST NORMAL_STYLE_DIRECTX, CONST NORMAL_STYLE_OPENGL, CONST OWNER_IMAGE, CONST OWNER_LAYER, CONST OWNER_MANAGER, CONST WORKFLOW_CLASSIC, CONST WORKFLOW_PBR_METALLIC_ROUGHNESS, display_name/0, duplicate/0, hatch_pattern/0, hatch_pattern=/1, materialType/0, metallic_factor/0, metallic_factor=/1, metallic_texture/0, metallic_texture=/1, metalness_enabled?/0, metalness_enabled=/1, name/0, name=/1, normal_enabled?/0, normal_enabled=/1, normal_scale/0, normal_scale=/1, normal_style/0, normal_style=/1, normal_texture/0, normal_texture=/1, owner_type/0, roughness_enabled?/0, roughness_enabled=/1, roughness_factor/0, roughness_factor=/1, roughness_texture/0, roughness_texture=/1, save_as/1, texture/0, texture=/1, use_alpha?/0, workflow/0, write_thumbnail/2

## Sketchup::Materials (14)
[]/1, add/*, add_observer/1, at/1, current/0, current=/1, each/0, length/0, load/1, purge_unused/0, remove/1, remove_observer/1, size/0, unique_name/1

## Sketchup::Menu (4)
add_item/*, add_separator/0, add_submenu/*, set_validation_proc/1

## Sketchup::Model (102)
abort_operation/0, active_entities/0, active_layer/0, active_layer=/1, active_path/0, active_path=/1, active_section_planes/0, active_view/0, add_diffusion_image/2, add_note/*, add_observer/1, attribute_dictionaries/0, attribute_dictionary/*, axes/0, behavior/0, bounds/0, classifications/0, clear_diffusion_image/0, close/*, close_active/0, commit_operation/0, CONST LOAD_STATUS_SUCCESS, CONST LOAD_STATUS_SUCCESS_MORE_RECENT, CONST Make, CONST MakeTrial, CONST ProLicensed, CONST ProTrial, CONST VERSION_2013, CONST VERSION_2014, CONST VERSION_2015, CONST VERSION_2016, CONST VERSION_2017, CONST VERSION_2018, CONST VERSION_2019, CONST VERSION_2020, CONST VERSION_2021, CONST VERSION_3, CONST VERSION_4, CONST VERSION_5, CONST VERSION_6, CONST VERSION_7, CONST VERSION_8, definitions/0, description/0, description=/1, diffusion_image_opacity/0, diffusion_image_opacity=/1, drawing_element_visible?/1, edit_transform/0, entities/0, environments/0, export/*, find_entity_by_id/*, find_entity_by_persistent_id/*, georeferenced?/0, get_attribute/*, get_datum/0, get_product_family/0, guid/0, hatch_patterns/0, import/*, instance_path_from_pid_path/1, latlong_to_point/1, layers/0, line_styles/0, list_datums/0, materials/0, merge/*, mipmapping?/0, mipmapping=/1, model_attribute_dictionaries/0, modified?/0, name/0, name=/1, number_faces/0, options/0, overlays/0, pages/0, path/0, place_component/*, point_to_latlong/1, point_to_utm/1, raytest/*, remove_observer/1, rendering_options/0, save/*, save_copy/*, save_thumbnail/1, select_tool/1, selection/0, set_attribute/*, set_datum/1, shadow_info/0, skpdoc/*, start_operation/*, styles/0, tags/0, tags=/1, title/0, tools/0, utm_to_point/1, valid?/0

## Sketchup::OptionsManager (5)
[]/1, each/0, keys/0, length/0, size/0

## Sketchup::OptionsProvider (14)
[]/1, []=/2, add_observer/1, each/0, each_key/0, each_pair/0, each_value/0, has_key?/1, key?/1, keys/0, length/0, name/0, remove_observer/1, size/0

## Sketchup::Overlay (11)
clone/0, description/0, description=/1, dup/0, enabled?/0, enabled=/1, initialize/*, name/0, overlay_id/0, source/0, valid?/0

## Sketchup::OverlaysManager (7)
[]/1, add/1, at, each/0, length, remove/1, size/0

## Sketchup::Page (50)
active_section_planes/0, axes/0, camera/0, delay_time/0, delay_time=/1, description/0, description=/1, environment/0, environment=/1, get_drawingelement_visibility/1, hidden_entities/0, include_in_animation?/0, include_in_animation=/1, label/0, layer_folders/0, layers/0, name/0, name=/1, rendering_options/0, set_drawingelement_visibility/2, set_visibility/2, shadow_info/0, style/0, transition_time/0, transition_time=/1, update/*, use_axes?/0, use_axes=/1, use_camera?/0, use_camera=/1, use_environment?/0, use_environment=/1, use_hidden?/0, use_hidden_geometry?/0, use_hidden_geometry=/1, use_hidden_layers?/0, use_hidden_layers=/1, use_hidden_objects?/0, use_hidden_objects=/1, use_hidden=/1, use_point_cloud_data?/0, use_point_cloud_data=/1, use_rendering_options?/0, use_rendering_options=/1, use_section_planes?/0, use_section_planes=/1, use_shadow_info?/0, use_shadow_info=/1, use_style?/0, use_style=/1

## Sketchup::Pages (24)
[]/1, add/*, add_matchphoto_page/*, add_observer/1, CONST ImageEmbedded, CONST ImageEmbeddedAndLinked, CONST ImageLinked, CONST UnitsNormalizedX, CONST UnitsNormalizedY, CONST UnitsPixels, each/0, erase/1, length/0, parent/0, remove_observer/1, reorder/2, selected_page/0, selected_page=/1, self.add_frame_change_observer/1, self.remove_frame_change_observer/1, show_frame_at/1, size/0, slideshow_time/0, unique_name/1

## Sketchup::PickHelper (20)
all_picked/0, best_picked/0, boundingbox_pick/*, CONST PICK_CROSSING, CONST PICK_INSIDE, count/0, depth_at/1, do_pick/*, element_at/1, init/*, leaf_at/1, path_at/1, pick_segment/*, picked_edge/0, picked_element/*, picked_face/0, test_point/*, transformation_at/1, view/0, window_pick/3

## Sketchup::Procedure (3)
clone/0, dup/0, initialize/4

## Sketchup::RegionalSettings (2)
self.decimal_separator/0, self.list_separator/0

## Sketchup::RenderingOptions (88)
[]/1, []=/2, add_observer/1, CONST ROPAssign, CONST ROPDrawHidden, CONST ROPDrawHiddenGeometry, CONST ROPDrawHiddenObjects, CONST ROPEditComponent, CONST ROPSectionDisplayTurnedOff, CONST ROPSetAOColor, CONST ROPSetAOColorEnabled, CONST ROPSetAODistance, CONST ROPSetAOEnabled, CONST ROPSetAOIntensity, CONST ROPSetAOMultiplier, CONST ROPSetBackgroundColor, CONST ROPSetConstructionColor, CONST ROPSetDepthQueEdges, CONST ROPSetDepthQueWidth, CONST ROPSetDisplayColorByLayer, CONST ROPSetDisplayDims, CONST ROPSetDisplayFog, CONST ROPSetDisplayInstanceAxes, CONST ROPSetDisplaySketchAxes, CONST ROPSetDisplayText, CONST ROPSetDisplayWatermarks, CONST ROPSetDrawBackEdges, CONST ROPSetDrawGround, CONST ROPSetDrawHorizon, CONST ROPSetDrawUnderground, CONST ROPSetEdgeColorMode, CONST ROPSetEdgeDisplayMode, CONST ROPSetEdgeType, CONST ROPSetExtendEdges, CONST ROPSetExtendLines, CONST ROPSetFaceColor, CONST ROPSetFaceColorMode, CONST ROPSetFogColor, CONST ROPSetFogDist, CONST ROPSetFogHint, CONST ROPSetFogUseBkColor, CONST ROPSetForegroundColor, CONST ROPSetGroundColor, CONST ROPSetGroundTransparency, CONST ROPSetHideConstructionGeometry, CONST ROPSetHideSpaces, CONST ROPSetHighlightColor, CONST ROPSetHorizonColor, CONST ROPSetJitterEdges, CONST ROPSetLineEndEdges, CONST ROPSetLineEndWidth, CONST ROPSetLineExtension, CONST ROPSetLineStyleEdges, CONST ROPSetLockedColor, CONST ROPSetMaterialTransparency, CONST ROPSetModelingGrid, CONST ROPSetModelTransparency, CONST ROPSetPhotomatchBackgroundOpacity, CONST ROPSetPhotomatchDrawBackground, CONST ROPSetPhotomatchDrawOverlay, CONST ROPSetPhotomatchOverlayOpacity, CONST ROPSetProfileEdges, CONST ROPSetProfilesOnlyEdges, CONST ROPSetProfileWidth, CONST ROPSetRenderMode, CONST ROPSetSectionActiveColor, CONST ROPSetSectionCutFilled, CONST ROPSetSectionCutWidth, CONST ROPSetSectionDefaultCutColor, CONST ROPSetSectionDefaultFillColor, CONST ROPSetSectionDisplayMode, CONST ROPSetSectionInactiveColor, CONST ROPSetSkyColor, CONST ROPSetTexture, CONST ROPSetTransparencyObsolete, CONST ROPSetXRayOpacity, CONST ROPTransparencySortMethod, each/0, each_key/0, each_pair/0, hatch_pattern/0, hatch_pattern=/1, keys/0, length/0, remove_observer/1, self.each_key/0, self.keys/0, size/0

## Sketchup::SectionPlane (10)
activate/0, active?/0, get_plane/0, hatch_pattern/0, hatch_pattern=/1, name/0, name=/1, set_plane/*, symbol/0, symbol=/1

## Sketchup::Selection (22)
[]/1, add, add_observer/1, at/1, clear/0, contains?/1, each/0, empty?/0, first/0, include?/1, invert/0, is_curve?, is_surface?, length/0, model/0, nitems/0, remove, remove_observer/1, shift/0, single_object?/0, size/0, toggle

## Sketchup::Set (10)
clear/0, contains?/1, delete/1, each/0, empty?/0, include?/1, insert/*, length/0, size/0, to_a/0

## Sketchup::ShadowInfo (12)
[]/1, []=/2, add_observer/1, each/0, each_key/0, each_pair/0, keys/0, length/0, remove_observer/1, self.each_key/0, self.keys/0, size/0

## Sketchup::Skp (1)
self.read_guid/1

## Sketchup::Snap (4)
direction/0, position/0, set/*, up/0

## Sketchup::Space (9)
add_hole/1, ceiling_plane/0, ceiling_plane=/1, name/0, name=/1, orientation_point/0, orientation_point=/1, perimeter_points/0, perimeter_points=/1

## Sketchup::Style (6)
description/0, description=/1, duplicate/0, name/0, name=/1, path/0

## Sketchup::Styles (13)
[]/1, active_style/0, active_style_changed/0, add_style/*, each/0, length/0, parent/0, purge_unused/0, remove_style/1, selected_style/0, selected_style=/1, size/0, update_selected_style/0

## Sketchup::Text (20)
arrow_type/0, arrow_type=/1, attached_to/0, attached_to=/1, display_leader?/0, display_leader=/1, font/0, font=/1, has_leader?/0, leader_type/0, leader_type=/1, line_weight/0, line_weight=/1, point/0, point=/1, set_text/1, text/0, text=/1, vector/0, vector=/1

## Sketchup::Texture (10)
average_color/0, filename/0, height/0, image_height/0, image_rep/*, image_width/0, size=/1, valid?/0, width/0, write/*

## Sketchup::TextureWriter (7)
count/0, filename/1, handle/*, length/0, load/*, write/*, write_all/*

## Sketchup::Tools (8)
active_tool/0, active_tool_id/0, active_tool_name/0, add_observer/1, model/0, pop_tool/0, push_tool/1, remove_observer/1

## Sketchup::UVHelper (2)
get_back_UVQ/1, get_front_UVQ/1

## Sketchup::Vertex (7)
common_edge/1, curve_interior?/0, edges/0, faces/0, loops/0, position/0, used_by?/1

## Sketchup::View (51)
add_observer/1, animation=/1, average_refresh_time/0, camera/0, camera=/1, center/0, CONST CORNER_BOTTOM_LEFT, CONST CORNER_BOTTOM_RIGHT, CONST CORNER_TOP_LEFT, CONST CORNER_TOP_RIGHT, corner/1, device_height/0, device_width/0, draw/*, draw_line, draw_lines, draw_points/*, draw_polyline, draw_text/*, draw2d/*, drawing_color=/1, dynamic=/1, field_of_view/0, field_of_view=/1, graphics_engine/0, guess_target/*, inference_locked?/0, inputpoint/*, invalidate/0, last_refresh_time/*, line_stipple=/1, line_width=/1, load_texture/1, lock_inference/*, model/0, pick_helper/*, pickray/*, pixels_to_model/2, refresh/0, release_texture/1, remove_observer/1, screen_coords/1, set_color_from_line/*, show_frame/*, text_bounds/*, tooltip=/1, vpheight/0, vpwidth/0, write_image/*, zoom/*, zoom_extents/0

## SKSocket (4)
self.add_socket_listener/0, self.connect/2, self.disconnect/0, self.write/1

## Test (27)
self.AutoClickTime/0, self.AutoClickTime=/1, self.AutoDragDistance/0, self.AutoDragDistance=/1, self.CorruptModel/0, self.cv_enabled?/0, self.cv_enabled=/1, self.diff/*, self.draw_horizontal_bubble_lines?/0, self.draw_horizontal_bubble_lines=/1, self.eat_mem/1, self.export_animation/*, self.FindFile/2, self.flip_images_for_origami/2, self.min_feature_culling_size?/0, self.min_feature_culling_size=/1, self.project_textures_from_photo/0, self.repair_model/*, self.send_extension_notification/*, self.show_error_dialog/0, self.show_fps/1, self.suppress_warnings?/0, self.suppress_warnings=/1, self.time_display/*, self.time_pick/*, self.ToolTips=/1, self.unregister_procedure/1

## Test::ExtensionNotificationType (5)
CONST EXTENSION_DISABLED, CONST EXTENSION_ENABLED, CONST EXTENSION_INSTALLED, CONST EXTENSION_UNINSTALLED, CONST EXTENSION_UPDATED

## Test::ExtensionResult (10)
CONST RESULT_FAILED, CONST RESULT_FAILED_ARCHIVE_READ, CONST RESULT_FAILED_DOWNLOAD, CONST RESULT_FAILED_NO_ARCHIVE_HANDLER, CONST RESULT_FAILED_REMOVE_FILES, CONST RESULT_FAILED_REMOVE_LICENSE, CONST RESULT_FAILED_UPDATE_LICENSE, CONST RESULT_FAILED_UPDATE_VERSION_ID, CONST RESULT_FAILED_WRITE_ACCESS, CONST RESULT_SUCCEEDED

## Transformation (17)
*/1, clone/0, identity?/0, initialize/*, inverse/0, invert!/0, origin/0, self.axes/*, self.interpolate/3, self.rotation/3, self.scaling/*, self.translation/1, set!/1, to_a/0, xaxis/0, yaxis/0, zaxis/0

## Transformation2d (12)
*/1, ==/1, clone/0, identity?/0, initialize/*, inverse/0, invert!/0, self.rotation/2, self.scaling/*, self.translation/1, set!/1, to_a/0

## UI (31)
self.add_context_menu_handler/0, self.beep/0, self.create_cursor/*, self.get_clipboard_data/0, self.inputbox/*, self.inspector_names/0, self.menu/*, self.messagebox/*, self.model_info_pages/0, self.openpanel, self.openURL/1, self.play_sound/1, self.preferences_pages/0, self.refresh_inspectors/0, self.refresh_toolbars, self.savepanel, self.scale_factor/*, self.select_directory/*, self.set_clipboard_data/1, self.set_cursor/1, self.set_toolbar_visible/2, self.show_extension_manager/0, self.show_extension_warehouse/*, self.show_inspector/1, self.show_model_info/*, self.show_preferences/*, self.start_timer/*, self.stop_timer/1, self.toolbar/1, self.toolbar_names/0, self.toolbar_visible?/1

## UI::ActionCallbackContext (2)
initialize/1, progress/1

## UI::Command (16)
extension/0, extension=/1, get_validation_proc/0, initialize/1, large_icon/0, large_icon=/1, menu_text/0, menu_text=/1, proc/0, set_validation_proc/0, small_icon/0, small_icon=/1, status_bar_text/0, status_bar_text=/1, tooltip/0, tooltip=/1

## UI::HtmlDialog (26)
add_action_callback/*, bring_to_front/0, center/0, close/0, CONST CEF_VERSION, CONST CHROME_VERSION, CONST STYLE_DIALOG, CONST STYLE_UTILITY, CONST STYLE_WINDOW, execute_script/1, get_content_size/0, get_position/0, get_size/0, hide/0, initialize/*, set_can_close/0, set_content_size/2, set_file/1, set_html/1, set_on_closed/0, set_position/2, set_size/2, set_url/1, show/0, show_modal/0, visible?/0

## UI::InputBox (12)
add_checkbox/*, add_description/1, add_dropdown/*, add_image/1, add_listbox/*, add_separator/*, add_slider/*, add_textbox/*, clone/0, dup/0, initialize/*, prompt/0

## UI::InputBox::ValueType (4)
CONST FLOAT, CONST INTEGER, CONST LENGTH, CONST STRING

## UI::Notification (12)
icon_name/0, icon_name=/1, icon_tooltip/0, icon_tooltip=/1, initialize/*, message/0, message=/1, on_accept/*, on_accept_title/0, on_dismiss/*, on_dismiss_title/0, show/0

## UI::Toolbar (12)
add_item/1, add_separator/0, each/0, get_last_state/0, hide/0, length/0, name/0, restore/0, self.new/1, show/0, size/0, visible?/0

## UI::WebDialog (32)
add_action_callback/1, allow_actions_from_host/1, bring_to_front/0, close/0, execute_script/1, get_default_dialog_color/0, get_element_value/1, initialize/*, max_height/0, max_height=/1, max_width/0, max_width=/1, min_height/0, min_height=/1, min_width/0, min_width=/1, navigation_buttons_enabled?/0, navigation_buttons_enabled=/1, post_url/2, screen_scale_factor/0, set_background_color/1, set_file/*, set_full_security/0, set_html/1, set_on_close/0, set_position/2, set_size/2, set_url/1, show/0, show_modal/0, visible?/0, write_image/*

## UTM (8)
initialize/*, to_a/0, to_latlong/0, to_s/0, x/0, y/0, zone_letter/0, zone_number/0

## Vector2d (33)
%/1, */1, -/1, []/1, []=/2, +/1, ==/1, angle_between/1, clone/0, cross/1, dot/1, initialize/*, inspect/0, length/0, length=/1, normalize!/0, normalize/0, parallel?/1, perpendicular?/1, reverse!/0, reverse/0, same_direction?/1, set!/*, to_a/0, to_s/0, transform!/1, transform/1, unit_vector?/0, valid?/0, x/0, x=/1, y/0, y=/1

## Vector3d (38)
%/1, */1, -/1, []/1, []=/2, +/1, </1, ==/1, angle_between/1, axes/0, clone/0, cross/1, dot/1, initialize/*, inspect/0, length/0, length=/1, normalize!/0, normalize/0, parallel?/1, perpendicular?/1, reverse!/0, reverse/0, samedirection?/1, self.linear_combination/*, set!/*, to_a/0, to_s/0, transform!/1, transform/1, unitvector?/0, valid?/0, x/0, x=/1, y/0, y=/1, z/0, z=/1

