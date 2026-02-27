#!/bin/bash
# Start the Rust backend in the background
echo "Starting Calendar Curator backend..."
export DATABASE_PATH=/app/data/calendars.json
./calendar-curator &
BACKEND_PID=$!

# Start the Next.js frontend
echo "Starting frontend..."
cd frontend
export NODE_ENV=production
export PORT=3000
export HOSTNAME="0.0.0.0"
node server.js --hostname $HOSTNAME &
FRONTEND_PID=$!

# Wait for both processes
wait $BACKEND_PID $FRONTEND_PID
