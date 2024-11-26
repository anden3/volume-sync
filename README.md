# Homework Assignment: Sound Control Synchronization

Create a Rust application that synchronizes system audio volume with a GUI interface. The application must reflect system volume changes made outside of it and allow users to adjust the volume through the app, propagating the changes back to the system.
The focus is to implement this functionality for **your current operating system**. Optionally, you may extend support to additional platforms if desired.

## Requirements

1. **Tauri Application:**
    - Create a simple GUI using Tauri with:
        - A slider for volume control (0% to 100%). 
        - A text display showing the current volume percentage.
    - When the slider is adjusted in the app:
        - Update the system's audio volume.
    - When the system's audio volume changes outside the app:
        - Reflect the change in the slider and text display.

2. **Native OS Integration:**
   - Use platform-specific libraries to get and set the system volume:
        - Windows: Use windows-rs and Core Audio APIs.
        - macOS: Use objc2 and CoreAudio APIs.
        - Android: Use ndk to interact with the AudioManager APIs.
   - Focus on implementing this functionality for your current OS. Optionally, implement support for additional platforms.

3. **Async Programming:**
   - Use tokio or another async runtime to:
        - Monitor system volume changes without blocking the UI.
        - Handle communication between the backend and frontend.

4. **Cross-Platform Conditional Compilation:**
   - Use cfg attributes to write OS-specific code.
   - If implementing for multiple platforms, ensure the application compiles and runs correctly

5. **Error Handling:**
   - Handle errors gracefully (e.g., permissions issues, unsupported APIs) and display an appropriate message in the app.


## Bonus Features (Optional)

1. Mute Control:
   - Add a mute/unmute button synchronized with the system.

2. Cross-Platform Implementation:
   - Extend the implementation to other operating systems.

3. Audio Feedback:
   - Play a test sound when adjusting the volume.

4. Tests:
   - Write tests for volume change logic or platform-specific integrations.


## Deliverables
- A GitHub repository (or zip file) containing:
   - Rust project files.
   - A README.md file with:
      - Setup and build instructions for the implemented OS.
      - Explanation of the approach and any challenges faced.
      - Instructions for building and running the app on other platforms (if cross-platform support is implemented).
