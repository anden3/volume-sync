import React, { useState, useEffect } from "react";
import "./App.css";

const App: React.FC = () => {
    const [volume, setVolume] = useState<number>(50);

    useEffect(() => {
        // Listen for volume changes
    }, []);


    const handleVolumeChange = async (newVolume: number) => {
        setVolume(newVolume);

        // Send the new volume to the backend
    };

    return (
        <div className="App">
            <header className="App-header">
                <h1>Volume Sync App</h1>
                <div className="volume-control">
                    <input
                        type="range"
                        min="0"
                        max="100"
                        value={volume}
                        onChange={(e) => handleVolumeChange(Number(e.target.value))}
                    />
                    <p>Current Volume: {volume}%</p>
                </div>
            </header>
        </div>
    );
};

export default App;