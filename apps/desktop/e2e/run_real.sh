#!/bin/bash
set -e

# Start Xvfb
echo "Starting Xvfb..."
Xvfb :99 -screen 0 1280x1024x24 &
XVFB_PID=$!
export DISPLAY=:99

# Start Fluxbox (Window Manager)
echo "Starting Fluxbox..."
fluxbox &
FLUXBOX_PID=$!

# Clean up on exit
cleanup() {
    echo "Cleaning up..."
    if [ -n "$APP_PID" ]; then kill $APP_PID 2>/dev/null || true; fi
    if [ -n "$FLUXBOX_PID" ]; then kill $FLUXBOX_PID 2>/dev/null || true; fi
    kill $XVFB_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "Starting Tauri app..."
APP_PATH="../../target/debug/desktop" # Relative to apps/desktop/e2e/

if [ ! -f "$APP_PATH" ]; then
    echo "Binary not found at $APP_PATH. Trying absolute..."
    APP_PATH=$(find ../../../target -name desktop -type f -executable | head -n 1)
fi

echo "Using binary: $APP_PATH"
$APP_PATH &
APP_PID=$!

echo "Waiting for window 'desktop'..."
count=0
while ! xdotool search --name "desktop"; do
    sleep 1
    count=$((count+1))
    if [ $count -ge 60 ]; then
        echo "Timed out waiting for window"
        # Take a screenshot of desktop anyway to debug
        mkdir -p ../screenshots
        scrot ../screenshots/timeout_debug.png
        exit 1
    fi
done

echo "Window found. Interacting..."
sleep 5 # Wait for full load
WID=$(xdotool search --name "desktop" | head -1)

# Activate window
xdotool windowactivate --sync $WID

# Focus and type
# Assume the window is focused.
xdotool type "Hello"
sleep 1
xdotool key Return

echo "Waiting for response..."
sleep 10 # Wait for stream

echo "Taking screenshot..."
mkdir -p ../screenshots
scrot ../screenshots/real_app_interaction.png

echo "Done."
