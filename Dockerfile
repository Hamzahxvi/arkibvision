FROM python:3.13-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libgl1 \
    libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

COPY pyproject.toml README.md ./
COPY config/ ./config/
COPY src/ ./src/

RUN pip install --no-cache-dir .

EXPOSE 8005

CMD ["python", "src/models/rojakface_mcp.py"]
