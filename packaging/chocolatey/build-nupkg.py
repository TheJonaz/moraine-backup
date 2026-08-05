#!/usr/bin/env python3
"""Build the Chocolatey .nupkg without `choco pack`, which is Windows-only.

A .nupkg is an OPC container: a zip holding the nuspec, the `tools/` payload and
the three bookkeeping parts (`[Content_Types].xml`, `_rels/.rels` and a
`.psmdcp` core-properties part) that NuGet clients expect. This writes those by
hand so the package can be built and pushed from Linux.

    ./packaging/chocolatey/build-nupkg.py

The .nupkg lands next to the nuspec. Push it with:

    curl -H "X-NuGet-ApiKey: $KEY" -T moraine.<version>.nupkg \
         https://push.chocolatey.org/api/v2/package
"""

import hashlib
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree

HERE = Path(__file__).resolve().parent
NUSPEC = HERE / "moraine.nuspec"
NS = "http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd"

# Fixed timestamp: the package should be byte-identical across rebuilds of the
# same sources, so a rebuild can be diffed against what was pushed.
ZIP_DATE = (2020, 1, 1, 0, 0, 0)

CONTENT_TYPES = """<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />
  <Default Extension="psmdcp" ContentType="application/vnd.openxmlformats-package.core-properties+xml" />
  <Default Extension="nuspec" ContentType="application/octet" />
  <Default Extension="ps1" ContentType="application/octet" />
  <Default Extension="txt" ContentType="application/octet" />
</Types>
"""

RELS = """<?xml version="1.0" encoding="utf-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Type="http://schemas.microsoft.com/packaging/2010/07/manifest" Target="/{nuspec}" Id="R{rid1}" />
  <Relationship Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="/{psmdcp}" Id="R{rid2}" />
</Relationships>
"""

PSMDCP = """<?xml version="1.0" encoding="utf-8"?>
<coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">
  <dc:creator>{authors}</dc:creator>
  <dc:description>{summary}</dc:description>
  <dc:identifier>{pkgid}</dc:identifier>
  <version>{version}</version>
  <keywords>{tags}</keywords>
</coreProperties>
"""


def text(metadata, tag):
    node = metadata.find(f"{{{NS}}}{tag}")
    if node is None or not node.text:
        sys.exit(f"error: <{tag}> is missing from {NUSPEC.name}")
    return node.text.strip()


def main():
    tree = ElementTree.parse(NUSPEC)
    root = tree.getroot()
    metadata = root.find(f"{{{NS}}}metadata")
    pkgid, version = text(metadata, "id"), text(metadata, "version")

    # The <files> element describes how to build the package; it has no meaning
    # once the files are in it, and `choco pack` drops it too.
    for files in root.findall(f"{{{NS}}}files"):
        root.remove(files)
    nuspec_blob = ElementTree.tostring(root, encoding="utf-8", xml_declaration=True)

    payload = sorted(p for p in (HERE / "tools").rglob("*") if p.is_file())
    if not payload:
        sys.exit("error: tools/ is empty — nothing to package")

    # Part names are arbitrary but must be stable per package, so derive them
    # from the manifest rather than a random GUID.
    digest = hashlib.sha256(nuspec_blob).hexdigest()
    psmdcp = f"package/services/metadata/core-properties/{digest[:32]}.psmdcp"

    out = HERE / f"{pkgid}.{version}.nupkg"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:

        def write(name, data):
            info = zipfile.ZipInfo(name, date_time=ZIP_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            z.writestr(info, data)

        write(f"{pkgid}.nuspec", nuspec_blob)
        for path in payload:
            write(f"tools/{path.relative_to(HERE / 'tools').as_posix()}", path.read_bytes())
        write("[Content_Types].xml", CONTENT_TYPES)
        write(
            "_rels/.rels",
            RELS.format(nuspec=f"{pkgid}.nuspec", psmdcp=psmdcp,
                        rid1=digest[32:48], rid2=digest[48:64]),
        )
        write(
            psmdcp,
            PSMDCP.format(authors=text(metadata, "authors"), summary=text(metadata, "summary"),
                          pkgid=pkgid, version=version, tags=text(metadata, "tags")),
        )

    print(f"{out.relative_to(Path.cwd()) if out.is_relative_to(Path.cwd()) else out}")
    for name in zipfile.ZipFile(out).namelist():
        print(f"  {name}")


if __name__ == "__main__":
    main()
