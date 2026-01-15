import React, { useState, useEffect } from 'react'
import AssemblyBrowser from './components/AssemblyBrowser'
import ClassExplorer from './components/ClassExplorer'
import InstanceViewer from './components/InstanceViewer'
import './App.css'

function App() {
  const [connected, setConnected] = useState(false)
  const [assemblies, setAssemblies] = useState([])
  const [selectedAssembly, setSelectedAssembly] = useState(null)
  const [selectedClass, setSelectedClass] = useState(null)
  const [selectedInstance, setSelectedInstance] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(false)

  // Connect to MTGA process
  const connectToMTGA = async () => {
    setLoading(true)
    setError(null)
    try {
      // This would call the Rust binary
      // For now, we'll simulate with a fetch to a local server
      const response = await fetch('http://localhost:8080/assemblies')
      const data = await response.json()

      setAssemblies(data.assemblies || [])
      setConnected(true)
    } catch (err) {
      setError(`Failed to connect: ${err.message}`)
      console.error('Connection error:', err)
    } finally {
      setLoading(false)
    }
  }

  // Load classes from selected assembly
  const loadAssembly = async (assemblyName) => {
    setLoading(true)
    setError(null)
    try {
      const response = await fetch(`http://localhost:8080/assembly/${encodeURIComponent(assemblyName)}/classes`)
      const data = await response.json()

      setSelectedAssembly({
        name: assemblyName,
        classes: data.classes || []
      })
      setSelectedClass(null)
      setSelectedInstance(null)
    } catch (err) {
      setError(`Failed to load assembly: ${err.message}`)
      console.error('Load assembly error:', err)
    } finally {
      setLoading(false)
    }
  }

  // Load class definition and find instances
  const loadClass = async (assemblyName, className) => {
    setLoading(true)
    setError(null)
    try {
      const response = await fetch(
        `http://localhost:8080/assembly/${encodeURIComponent(assemblyName)}/class/${encodeURIComponent(className)}`
      )
      const data = await response.json()

      setSelectedClass(data)
      setSelectedInstance(null)
    } catch (err) {
      setError(`Failed to load class: ${err.message}`)
      console.error('Load class error:', err)
    } finally {
      setLoading(false)
    }
  }

  // Load instance data
  const loadInstance = async (address) => {
    setLoading(true)
    setError(null)
    try {
      const response = await fetch(`http://localhost:8080/instance/${address}`)
      const data = await response.json()

      setSelectedInstance(data)
    } catch (err) {
      setError(`Failed to load instance: ${err.message}`)
      console.error('Load instance error:', err)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="app">
      <header className="header">
        <h1>MTGA Reader Debug UI</h1>
        <div className="header-actions">
          {!connected ? (
            <button
              onClick={connectToMTGA}
              disabled={loading}
              className="btn btn-primary"
            >
              {loading ? 'Connecting...' : 'Connect to MTGA'}
            </button>
          ) : (
            <div className="status">
              <span className="status-indicator active"></span>
              Connected
            </div>
          )}
        </div>
      </header>

      {error && (
        <div className="error-banner">
          <strong>Error:</strong> {error}
          <button onClick={() => setError(null)} className="close-btn">×</button>
        </div>
      )}

      {connected && (
        <div className="main-content">
          <AssemblyBrowser
            assemblies={assemblies}
            selectedAssembly={selectedAssembly}
            onSelectAssembly={loadAssembly}
            loading={loading}
          />

          {selectedAssembly && (
            <ClassExplorer
              assembly={selectedAssembly}
              selectedClass={selectedClass}
              onSelectClass={(className) => loadClass(selectedAssembly.name, className)}
              onSelectInstance={loadInstance}
              loading={loading}
            />
          )}

          {selectedInstance && (
            <InstanceViewer
              instance={selectedInstance}
              onNavigate={loadInstance}
              loading={loading}
            />
          )}
        </div>
      )}

      {!connected && !error && !loading && (
        <div className="empty-state">
          <div className="empty-state-content">
            <h2>Welcome to MTGA Reader Debug UI</h2>
            <p>Connect to a running MTGA process to start browsing game data</p>
            <p className="note">
              Note: Make sure the MTGA reader HTTP server is running on port 8080
            </p>
          </div>
        </div>
      )}
    </div>
  )
}

export default App
