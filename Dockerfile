# Unified Dockerfile for both frontend and backend
FROM node:20-slim AS frontend-builder
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
WORKDIR /app/frontend

# Copy package files and install dependencies
COPY web/package.json web/pnpm-lock.json* ./
RUN pnpm i

# Copy frontend source and build
COPY web/ .
RUN pnpm build

# Rust backend stage
FROM rust:slim AS backend-builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files first for better caching
COPY app/Cargo.toml app/Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release

# Remove the dummy artifacts and source - this is crucial!
RUN rm -rf src target/release/deps/Calendar_Curator* target/release/Calendar-Curator*

# Copy the actual source code
COPY app/src ./src

# Force rebuild of the actual binary
RUN cargo build --release

# Final runtime stage combining both
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js for serving the frontend
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

WORKDIR /app

# Copy the Rust backend binary
COPY --from=backend-builder /app/target/release/Calendar-Curator /app/calendar-curator
RUN chgrp 0 /app/calendar-curator && chmod g+rx /app/calendar-curator

# Copy the built frontend
COPY --from=frontend-builder /app/frontend/.next/standalone ./frontend/
COPY --from=frontend-builder /app/frontend/.next/static ./frontend/.next/static
COPY --from=frontend-builder /app/frontend/public ./frontend/public
RUN chgrp -R 0 ./frontend && chmod -R g+rwX ./frontend

COPY ./start.sh /app/start.sh
RUN chgrp 0 /app/start.sh && chmod g+rx /app/start.sh

# Create data directory for persistent storage
RUN mkdir -p /app/data && chgrp 0 /app/data && chmod g+rwX /app/data

RUN chgrp -R 0 /app \
    && chmod -R g+rwX /app

# Set environment variables
ENV DATABASE_PATH=/app/data/calendars.json
ENV NODE_ENV=production

# Expose both ports
EXPOSE 3000

CMD ["/app/start.sh"]
