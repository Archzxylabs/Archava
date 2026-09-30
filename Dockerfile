FROM python:3.12-slim-bookworm

# Install system dependencies and Node.js 20
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install Node production dependencies
COPY package*.json ./
RUN npm ci --omit=dev || npm install --omit=dev

# Install Python requirements
COPY backend/requirements.txt ./backend/
RUN pip install --no-cache-dir -r backend/requirements.txt

# Copy server, backend, and runtime files
COPY server ./server
COPY backend ./backend
COPY public ./public
COPY entrypoint.sh ./

RUN mkdir -p data && chmod +x entrypoint.sh

ENV PORT=5002
EXPOSE 5002

CMD ["./entrypoint.sh"]
