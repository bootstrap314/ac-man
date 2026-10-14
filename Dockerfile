# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

# Refresh these verified multi-platform digests when updating the toolchain/base.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS source
WORKDIR /build
# .dockerignore is an allowlist: no user credentials, data or app bundles.
COPY . .

FROM source AS test
RUN cargo test --locked --all-targets

FROM source AS build
RUN cargo build --locked --release --bin ac-man \
    && install -D -m 0755 target/release/ac-man /out/ac-man \
    && install -d -m 0700 /out/data \
    && install -D -m 0644 "$(gcc -print-file-name=libgcc_s.so.1)" /out/lib/libgcc_s.so.1 \
    && install -D -m 0644 /usr/share/doc/libgcc-s1/copyright /out/libgcc-copyright \
    && dpkg-query -s libgcc-s1 > /out/libgcc-status

# Preferred Docker use: export the Linux executable for use on a Linux host.
# docker buildx build --target cli --output type=local,dest=dist/docker .
FROM scratch AS cli
COPY --from=build /out/ac-man /ac-man

# BuildKit skips this stage for test/CLI exports. Runtime redistribution requires
# dependency notices: run `make licenses` first (included in make docker-build).
FROM scratch AS notices
COPY LICENSE THIRD_PARTY_NOTICES.md licenses/DEPENDENCIES.html /notices/

# Optional runtime for help/local commands. Saved login requires an unlocked
# user-session Secret Service; this image does not start or expose that service.
# Rust uses rustls, so this runtime needs libc and libgcc, but no OpenSSL or shell.
FROM gcr.io/distroless/base-nossl-debian12:nonroot@sha256:be40c00dfabd86576d92666e87e406714d5618342de1a0c213ad232de255172e AS runtime
LABEL org.opencontainers.image.licenses="Apache-2.0"
COPY --from=build /out/ac-man /usr/local/bin/ac-man
COPY --from=build /out/lib/ /usr/local/lib/
# Retain package metadata and copyright so vulnerability and license scans cover
# the extra runtime library copied from the pinned builder.
COPY --from=build /out/libgcc-status /var/lib/dpkg/status.d/libgcc-s1
COPY --from=build /out/libgcc-copyright /usr/share/doc/libgcc-s1/copyright
COPY --from=build --chown=10001:10001 /out/data /data
COPY --from=notices /notices/ /usr/share/doc/ac-man/
ENV AC_MAN_DATA_DIR=/data
ENV LD_LIBRARY_PATH=/usr/local/lib
USER 10001:10001
WORKDIR /data
ENTRYPOINT ["/usr/local/bin/ac-man"]
CMD ["--help"]
