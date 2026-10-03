FROM node:22-bookworm-slim AS dependencies
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

FROM dependencies AS frontend
COPY index.html tsconfig.json vite.config.ts ./
COPY src ./src
COPY public ./public
RUN npm run build

FROM dependencies AS production-dependencies
RUN npm prune --omit=dev

FROM python:3.12-slim-bookworm AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libopus0 ffmpeg \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=dependencies /usr/local/bin/node /usr/local/bin/node
COPY --from=production-dependencies /app/node_modules ./node_modules
COPY backend/requirements.txt ./backend/
RUN pip install --no-cache-dir -r backend/requirements.txt
COPY server ./server
COPY backend ./backend
COPY --from=frontend /app/dist ./dist
COPY entrypoint.sh ./
RUN mkdir -p /app/data && chmod +x entrypoint.sh

ENV NODE_ENV=production \
    PYTHONUNBUFFERED=1 \
    PORT=5002 \
    ARCHAVA_STATE_DIR=/app/data \
    ARCHAVA_WORKER_HEALTH_URL=http://127.0.0.1:8081/
EXPOSE 5002
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD node -e "fetch('http://127.0.0.1:'+process.env.PORT+'/api/ready').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["./entrypoint.sh"]
