#!/bin/sh

WORK_DIR="${PWD}"

SRC="${PKG_NAME}-${PKG_VERSION}-${target_platform}"

if test -f "${SRC}.zip"; then
    ( cd "$PREFIX" && unzip -n "${WORK_DIR}/${SRC}.zip" )
    // Delete extra stuff Macs apparently stuffed into zip files:
    rm -rf "$PREFIX/__MACOSX" || true
elif test -f "${SRC}.tar.gz"; then
    ( cd "$PREFIX" && tar -xzf "${WORK_DIR}/${SRC}.tar.gz" )
elif test -f "${SRC}.tar.xz"; then
    ( cd "$PREFIX" && tar -xJf "${WORK_DIR}/${SRC}.tar.xz" )
elif test -f "${SRC}.gz"; then
    ( cd "$PREFIX" && cat "${WORK_DIR}/${SRC}.gz" | gunzip > "${PREFIX}/${PKG_NAME}" )
    chmod 755 "${PREFIX}/${PKG_NAME}"
elif test -f "${SRC}.xz"; then
    ( cd "$PREFIX" && cat "${WORK_DIR}/${SRC}.xz" | unxz > "${PREFIX}/${PKG_NAME}" )
    chmod 755 "${PREFIX}/${PKG_NAME}"
elif test -f "${SRC}.zst"; then
    ( cd "$PREFIX" && cat "${WORK_DIR}/${SRC}.zst" | unzstd > "${PREFIX}/${PKG_NAME}" )
    chmod 755 "${PREFIX}/${PKG_NAME}"
elif test -f "${WORK_DIR}/${SRC}.exe"; then
    mkdir "${PREFIX}/bin"
    cp "${WORK_DIR}/${SRC}.exe" "${PREFIX}/bin/${PKG_NAME}.exe"
    chmod 755 "${PREFIX}/bin/${PKG_NAME}.exe"
elif test -f "${WORK_DIR}/${SRC}"; then
    cp "${WORK_DIR}/${SRC}" "${PREFIX}/${PKG_NAME}"
    chmod 755 "${PREFIX}/${PKG_NAME}"
else
    echo "${SRC} not found, not a file, not a zip, not an exe, not a tarball, not compressed"
    echo "Work directory contents is:"
    ls -alF "${WORK_DIR}"
    exit 1
fi

pushd "$PREFIX" || exit 3

shopt -s dotglob

# Move everything out of a "foo-arch-version" folder
while [ $(find . -mindepth 1 -maxdepth 1 -type d -not -name conda-meta | wc -l) -eq 1 ]; do
    if test -d "bin"; then
        echo "Found only a bin subdir, this looks good"
        break
    else
        # move everything up a level, using a temp name to avoid
        # conflicts when the directory contains a file with the same name
        SUBDIR=$(find . -mindepth 1 -maxdepth 1 -type d -not -name conda-meta)
        TMPNAME=".strip-$$"
        mv "${SUBDIR}" "${TMPNAME}"
        mv "${TMPNAME}"/* . || true
        rmdir "${TMPNAME}"
    fi
done

# Move all executable files into bin
mkdir -p bin
mkdir -p extras

for f in *; do
    if test -f "${f}"; then
        if file "${f}" | grep "executable"; then
            chmod 755 "${f}"
        fi

        if test -x "${f}"; then
            mv "${f}" bin
        else
            case "$f" in
            *.exe|*.bat|*.com)
                mv "${f}" bin
                ;;
            *)
                mv "${f}" extras
                ;;
            esac
        fi
    elif test -d "${f}"; then
        case "${f}" in
        conda-meta|bin|etc|include|lib|man|share|ssl|extras)
            ;;
        *)
            mv "${f}" extras
        esac
    fi
done

cd "${PREFIX}/bin"

for f in *; do
    if [[ "$f" == *"-${PKG_VERSION}"* ]]; then
        short="${f%%-*}"
        mv "${f}" "${short}"
    fi
done

shopt -u dotglob
