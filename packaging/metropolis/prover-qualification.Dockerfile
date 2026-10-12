FROM ghcr.io/astral-sh/uv:0.9.30@sha256:538e0b39736e7feae937a65983e49d2ab75e1559d35041f9878b7b7e51de91e4 AS uv
FROM rust:1.96.0-bookworm@sha256:5e2214abe154fe26e39f64488952e5c991eeed1d6d6da7cc8381ae83927f0cfc

COPY --from=uv /uv /usr/local/bin/uv
ENV UV_LINK_MODE=copy
ENV UV_PYTHON_DOWNLOADS=never
RUN uv venv --python /usr/bin/python3 /opt/qualification \
    && uv pip install --python /opt/qualification/bin/python pytest==9.1.1 hatchling==1.32.4
ENV PATH="/opt/qualification/bin:${PATH}"
