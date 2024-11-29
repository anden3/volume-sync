import React, { useState, useEffect } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import "./App.css";

const App: React.FC = () => {
    const [volume, setVolume] = useState<number | null>(0.2);

    useEffect(() => {
        // Listen for volume changes
        listen<number | null>('system-volume-changed', (event) => {
            setVolume(event.payload);
        });
    }, []);


    const handleVolumeChange = async (newVolume: number) => {
        setVolume(newVolume);

        // Send the new volume to the backend
        emit('web-volume-changed', newVolume);
    };

    function VolumeControl() {
        if (volume === null) {
            return <p>No input devices detected.</p>;
        } else {
            return (
                <div className="volume-control">
                    <input
                        type="range"
                        min="0"
                        max="1"
                        step="0.01"
                        value={volume}
                        onChange={(e) => handleVolumeChange(Number(e.target.value))}
                    />
                    <p>Current Volume: {Math.round(volume * 100.0)}%</p>
                </div>
            );
        }
    }

    return (
        <div className="App">
            <header className="App-header">
                <h1>Volume Sync App</h1>
                <VolumeControl />
            </header>
        </div>
    );
};

export default App;