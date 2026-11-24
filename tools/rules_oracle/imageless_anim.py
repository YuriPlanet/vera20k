"""Original AnimType admission/bounds when named images are absent.

This declares only inputs; the existing anim_theater_flags reader owns execution.
Use python -m tools.rules_oracle.imageless_anim --check (or --write).
"""
from pathlib import Path

from tools.native_oracle import finish_vectors
from tools.rules_oracle.anim_theater_flags import metadata as reader_metadata, run_case


def cases():
    return [
        dict(case="missing_image_default_bounds", name="INVISIBLE", theater=2,
             art_sections={"INVISIBLE": {"Normalized": "no"}}, available_images={}),
        dict(case="missing_image_explicit_end_lazy_loop", name="EXPLICIT", theater=2,
             art_sections={"EXPLICIT": {"End": "7", "LoopEnd": "-1"}}, available_images={}),
        dict(case="missing_image_lazy_end_explicit_loop", name="LAZY", theater=2,
             art_sections={"LAZY": {"End": "-1", "LoopEnd": "7"}}, available_images={}),
    ]


def generate():
    return dict(schema=1, cases=[run_case(case) for case in cases()])


def metadata():
    result = reader_metadata()
    result["scope"] = "Original null-image AnimType admission and explicit/lazy bounds"
    result["assumptions"] = [
        "Three fresh synthetic types with declared empty image catalogs. Full original constructor, ART reader, theater postpass and GetImage execute through the existing anim_theater_flags reader.",
        "NULL is supplied only at the existing LoadFileFromMIX transport seam; image pointers and frame bounds are never injected. Original text and AnimType vtable bytes remain guarded by that reader.",
        "Default/explicit/lazy End and LoopEnd results are native outputs. No full AnimClass lifetime, particle world, archive I/O, malformed SHP decoding or rendered parity is claimed.",
    ]
    return result


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata,
                   source_paths={"producer": Path(__file__),
                                 "shared_reader": Path(__file__).with_name("anim_theater_flags.py")})
