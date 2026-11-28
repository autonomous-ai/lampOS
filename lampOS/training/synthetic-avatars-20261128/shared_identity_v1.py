"""Source-only shared A00–A23 construction recipe; media seal is separate.

Use the pinned MPFB 2.0.18 extension and identity-parameters-v1.json.
The caller supplies the initialized services and local asset roots.
No fitting, random sampling, external paths, or model inference occurs here.
"""
from pathlib import Path

GLASSES_NAME = "spamrakuen_sagerfrogs_glasses_01"


def construct_identity(human_service, target_service, mpfb_data_root,
                       core_asset_root, glasses_asset_root, parameters):
    """Build one exact neutral identity and its rig/accessories in Blender.

    Preserve JSON insertion order for the nine facial target applications.
    Camera, head/shoulder posing, illumination, and rendering are separate.
    Returned base mesh retains the original 19,158-vertex indexing; its
    helper mask hides non-skin geometry during rendering.
    """
    data = Path(mpfb_data_root)
    core = Path(core_asset_root)
    glasses = Path(glasses_asset_root)
    human = human_service.create_human(scale=0.1, macro_detail_dict=parameters["macro"])
    for relative_target, weight in parameters["targets"].items():
        target_service.load_target(human, str(data / "targets" / relative_target), weight=weight)
    rig = human_service.add_builtin_rig(human, "default", import_weights=True)
    skin_name = parameters["skin_asset"]
    human_service.set_character_skin(
        str(core / "skins" / skin_name / (skin_name + ".mhmat")), human,
        skin_type="GAMEENGINE")
    assets = [
        (core / "eyes/low-poly/low-poly.mhclo", "Eyes"),
        (core / "eyebrows/eyebrow001/eyebrow001.mhclo", "Eyebrows"),
        (core / "eyelashes/eyelashes01/eyelashes01.mhclo", "Eyelashes"),
        (core / "clothes/male_casualsuit06/male_casualsuit06.mhclo", "Clothes"),
    ]
    hair_name = {"short": "short04", "long": "long01"}.get(parameters["hair_class"])
    if hair_name:
        assets.append((core / "hair" / hair_name / (hair_name + ".mhclo"), "Hair"))
    if parameters["glasses"]:
        assets.append((glasses / "clothes" / GLASSES_NAME / (GLASSES_NAME + ".mhclo"), "Clothes"))
    for asset_path, asset_type in assets:
        human_service.add_mhclo_asset(str(asset_path), human, asset_type=asset_type,
                                     subdiv_levels=0, material_type="GAMEENGINE")
    return human, rig
