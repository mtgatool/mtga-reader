import React, { useState, useEffect, useCallback, useRef } from 'react'
import AssemblyBrowser from './components/AssemblyBrowser'
import ClassExplorer from './components/ClassExplorer'
import InstanceViewer from './components/InstanceViewer'
import PathBreadcrumb from './components/PathBreadcrumb'
import './App.css'

// URL Path structure:
// /#/assembly/{name}/class/{className}/static/{fieldName}/field/{fieldName}/field/{fieldName}...
// Each segment represents a navigation step through memory

function parseUrlPath() {
  const hash = window.location.hash.slice(1) // Remove '#'
  if (!hash || hash === '/') return { segments: [] }

  const parts = hash.split('/').filter(Boolean)
  const segments = []

  let i = 0
  while (i < parts.length) {
    const type = parts[i]
    const value = decodeURIComponent(parts[i + 1] || '')

    if (type === 'assembly' && value) {
      segments.push({ type: 'assembly', value })
      i += 2
    } else if (type === 'class' && value) {
      segments.push({ type: 'class', value })
      i += 2
    } else if (type === 'static' && value) {
      segments.push({ type: 'static', value })
      i += 2
    } else if (type === 'field' && value) {
      segments.push({ type: 'field', value })
      i += 2
    } else {
      i++ // Skip unknown segments
    }
  }

  return { segments }
}

function buildUrlPath(segments) {
  if (!segments || segments.length === 0) return '#/'
  return '#/' + segments.map(s => `${s.type}/${encodeURIComponent(s.value)}`).join('/')
}

function App() {
  const [connected, setConnected] = useState(false)
  const [assemblies, setAssemblies] = useState([])
  const [selectedAssembly, setSelectedAssembly] = useState(null)
  const [selectedClass, setSelectedClass] = useState(null)
  const [selectedInstance, setSelectedInstance] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(false)

  // Navigation path state
  const [navigationPath, setNavigationPath] = useState([])
  const [pathError, setPathError] = useState(null) // { message, resolvedSegments, failedSegment }
  const isResolvingPath = useRef(false)
  const pendingPathResolution = useRef(null) // Store assemblies for deferred path resolution

  // Connect to MTGA process
  const connectToMTGA = async () => {
    setLoading(true)
    setError(null)
    try {
      // This would call the Rust binary
      // For now, we'll simulate with a fetch to a local server
      const response = await fetch('http://localhost:8080/assemblies')
      const data = await response.json()

      const assemblyList = data.assemblies || []
      setAssemblies(assemblyList)
      setConnected(true)

      // Store assemblies for deferred path resolution (will be picked up by useEffect)
      if (assemblyList.length > 0 && parseUrlPath().segments.length > 0) {
        pendingPathResolution.current = assemblyList
      }
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
      return data
    } catch (err) {
      setError(`Failed to load instance: ${err.message}`)
      console.error('Load instance error:', err)
      return null
    } finally {
      setLoading(false)
    }
  }

  // Update URL when navigation path changes (without triggering hashchange)
  const updateUrl = useCallback((segments) => {
    const newUrl = buildUrlPath(segments)
    if (window.location.hash !== newUrl.slice(1)) {
      window.history.pushState(null, '', newUrl)
    }
  }, [])

  // Navigate via path and update URL
  const navigateToAssembly = useCallback(async (assemblyName, updateUrlFlag = true) => {
    setLoading(true)
    setError(null)
    setPathError(null)
    try {
      const response = await fetch(`http://localhost:8080/assembly/${encodeURIComponent(assemblyName)}/classes`)
      const data = await response.json()

      const newPath = [{ type: 'assembly', value: assemblyName }]
      setSelectedAssembly({ name: assemblyName, classes: data.classes || [] })
      setSelectedClass(null)
      setSelectedInstance(null)
      setNavigationPath(newPath)

      if (updateUrlFlag) {
        updateUrl(newPath)
      }

      return { success: true, data }
    } catch (err) {
      setError(`Failed to load assembly: ${err.message}`)
      return { success: false, error: err.message }
    } finally {
      setLoading(false)
    }
  }, [updateUrl])

  const navigateToClass = useCallback(async (assemblyName, className, updateUrlFlag = true) => {
    setLoading(true)
    setError(null)
    setPathError(null)
    try {
      const response = await fetch(
        `http://localhost:8080/assembly/${encodeURIComponent(assemblyName)}/class/${encodeURIComponent(className)}`
      )
      const data = await response.json()

      const newPath = [
        { type: 'assembly', value: assemblyName },
        { type: 'class', value: className }
      ]
      setSelectedClass(data)
      setSelectedInstance(null)
      setNavigationPath(newPath)

      if (updateUrlFlag) {
        updateUrl(newPath)
      }

      return { success: true, data }
    } catch (err) {
      setError(`Failed to load class: ${err.message}`)
      return { success: false, error: err.message }
    } finally {
      setLoading(false)
    }
  }, [updateUrl])

  const navigateToStaticField = useCallback(async (assemblyName, className, fieldName, classAddress, updateUrlFlag = true) => {
    setLoading(true)
    setError(null)
    setPathError(null)
    try {
      // Read the static field value to get its address
      const fieldResponse = await fetch(
        `http://localhost:8080/class/0x${classAddress.toString(16)}/field/${encodeURIComponent(fieldName)}`
      )
      const fieldData = await fieldResponse.json()

      if (fieldData.type !== 'pointer' || !fieldData.address || fieldData.address === 0) {
        throw new Error(`Static field "${fieldName}" is not a valid pointer`)
      }

      // Load the instance
      const instanceResponse = await fetch(`http://localhost:8080/instance/0x${fieldData.address.toString(16)}`)
      const instanceData = await instanceResponse.json()

      const newPath = [
        { type: 'assembly', value: assemblyName },
        { type: 'class', value: className },
        { type: 'static', value: fieldName }
      ]

      setSelectedInstance(instanceData)
      setNavigationPath(newPath)

      if (updateUrlFlag) {
        updateUrl(newPath)
      }

      return { success: true, data: instanceData, address: fieldData.address }
    } catch (err) {
      setError(`Failed to navigate to static field: ${err.message}`)
      return { success: false, error: err.message }
    } finally {
      setLoading(false)
    }
  }, [updateUrl])

  const navigateToInstanceField = useCallback(async (currentAddress, fieldName, currentPath, updateUrlFlag = true) => {
    setLoading(true)
    setError(null)
    setPathError(null)
    try {
      // Read the field value to get its address
      const fieldResponse = await fetch(
        `http://localhost:8080/instance/0x${currentAddress.toString(16)}/field/${encodeURIComponent(fieldName)}`
      )
      const fieldData = await fieldResponse.json()

      if (fieldData.type !== 'pointer' || !fieldData.address || fieldData.address === 0) {
        throw new Error(`Field "${fieldName}" is not a valid pointer (type: ${fieldData.type}, address: ${fieldData.address})`)
      }

      // Load the instance at that address
      const instanceResponse = await fetch(`http://localhost:8080/instance/0x${fieldData.address.toString(16)}`)
      const instanceData = await instanceResponse.json()

      const newPath = [...currentPath, { type: 'field', value: fieldName }]

      setSelectedInstance(instanceData)
      setNavigationPath(newPath)

      if (updateUrlFlag) {
        updateUrl(newPath)
      }

      return { success: true, data: instanceData, address: fieldData.address }
    } catch (err) {
      setError(`Failed to navigate to field: ${err.message}`)
      return { success: false, error: err.message }
    } finally {
      setLoading(false)
    }
  }, [updateUrl])

  // Navigate to instance by address - this adds to current path as a special "address" segment
  const navigateToInstanceByAddress = useCallback(async (address, fieldNameHint = null) => {
    setLoading(true)
    setError(null)
    setPathError(null)
    try {
      const addressHex = typeof address === 'number' ? `0x${address.toString(16)}` : address
      const response = await fetch(`http://localhost:8080/instance/${addressHex}`)
      const data = await response.json()

      // If we have a field name hint, add it as a field segment
      if (fieldNameHint && navigationPath.length > 0) {
        const newPath = [...navigationPath, { type: 'field', value: fieldNameHint }]
        setNavigationPath(newPath)
        updateUrl(newPath)
      }

      setSelectedInstance(data)
      return { success: true, data }
    } catch (err) {
      setError(`Failed to load instance: ${err.message}`)
      return { success: false, error: err.message }
    } finally {
      setLoading(false)
    }
  }, [navigationPath, updateUrl])

  // Resolve a path from URL - called on initial load or when URL changes
  const resolvePathFromUrl = useCallback(async (assembliesData) => {
    if (isResolvingPath.current) return
    isResolvingPath.current = true

    const { segments } = parseUrlPath()
    if (segments.length === 0) {
      isResolvingPath.current = false
      return
    }

    setLoading(true)
    setError(null)
    setPathError(null)

    const resolvedSegments = []
    let currentAddress = null
    let assemblyName = null
    let className = null
    let classAddress = null

    try {
      for (let i = 0; i < segments.length; i++) {
        const segment = segments[i]

        if (segment.type === 'assembly') {
          assemblyName = segment.value
          // Check if assembly exists
          if (!assembliesData.find(a => a === assemblyName)) {
            throw { message: `Assembly "${assemblyName}" not found`, segment, resolvedSegments }
          }

          const result = await navigateToAssembly(assemblyName, false)
          if (!result.success) {
            throw { message: result.error, segment, resolvedSegments }
          }
          resolvedSegments.push(segment)

        } else if (segment.type === 'class') {
          if (!assemblyName) {
            throw { message: 'Class segment without assembly', segment, resolvedSegments }
          }
          className = segment.value
          const result = await navigateToClass(assemblyName, className, false)
          if (!result.success) {
            throw { message: result.error, segment, resolvedSegments }
          }
          classAddress = result.data.address
          resolvedSegments.push(segment)

        } else if (segment.type === 'static') {
          if (!className || !classAddress) {
            throw { message: 'Static segment without class', segment, resolvedSegments }
          }
          const result = await navigateToStaticField(assemblyName, className, segment.value, classAddress, false)
          if (!result.success) {
            throw { message: result.error, segment, resolvedSegments }
          }
          currentAddress = result.address
          resolvedSegments.push(segment)

        } else if (segment.type === 'field') {
          if (!currentAddress) {
            throw { message: 'Field segment without current instance', segment, resolvedSegments }
          }
          const result = await navigateToInstanceField(currentAddress, segment.value, resolvedSegments, false)
          if (!result.success) {
            throw { message: result.error, segment, resolvedSegments }
          }
          currentAddress = result.address
          resolvedSegments.push(segment)
        }
      }

      // Successfully resolved entire path
      setNavigationPath(resolvedSegments)
      updateUrl(resolvedSegments)

    } catch (pathErr) {
      // Path resolution failed at some point
      console.error('Path resolution error:', pathErr)
      setPathError({
        message: pathErr.message,
        resolvedSegments: pathErr.resolvedSegments || resolvedSegments,
        failedSegment: pathErr.segment
      })

      // Update URL to only show resolved segments
      if (pathErr.resolvedSegments && pathErr.resolvedSegments.length > 0) {
        setNavigationPath(pathErr.resolvedSegments)
        updateUrl(pathErr.resolvedSegments)
      }
    } finally {
      setLoading(false)
      isResolvingPath.current = false
    }
  }, [navigateToAssembly, navigateToClass, navigateToStaticField, navigateToInstanceField, updateUrl])

  // Navigate to a specific segment in the breadcrumb path
  const navigateToPathSegment = useCallback(async (segmentIndex) => {
    const targetPath = navigationPath.slice(0, segmentIndex + 1)
    const lastSegment = targetPath[targetPath.length - 1]

    setPathError(null)

    if (lastSegment.type === 'assembly') {
      await navigateToAssembly(lastSegment.value)
    } else if (lastSegment.type === 'class') {
      const assemblySegment = targetPath.find(s => s.type === 'assembly')
      if (assemblySegment) {
        await navigateToAssembly(assemblySegment.value, false)
        await navigateToClass(assemblySegment.value, lastSegment.value)
      }
    } else {
      // For static/field segments, we need to re-resolve the path up to that point
      // Set the path and let the URL change trigger a re-resolve
      const newUrl = buildUrlPath(targetPath)
      window.location.hash = newUrl.slice(1)
    }
  }, [navigationPath, navigateToAssembly, navigateToClass])

  // Handle browser back/forward navigation
  useEffect(() => {
    const handleHashChange = () => {
      if (!isResolvingPath.current && connected && assemblies.length > 0) {
        resolvePathFromUrl(assemblies)
      }
    }

    window.addEventListener('hashchange', handleHashChange)
    return () => window.removeEventListener('hashchange', handleHashChange)
  }, [connected, assemblies, resolvePathFromUrl])

  // Handle deferred path resolution after connection
  useEffect(() => {
    if (pendingPathResolution.current && connected) {
      const assembliesData = pendingPathResolution.current
      pendingPathResolution.current = null
      resolvePathFromUrl(assembliesData)
    }
  }, [connected, resolvePathFromUrl])

  // Handler for navigating to a field from the InstanceViewer
  const handleInstanceFieldNavigate = (address, fieldName) => {
    // If we have a current path and the fieldName is provided, navigate by field name
    if (fieldName && navigationPath.length > 0 && selectedInstance) {
      navigateToInstanceField(selectedInstance.address, fieldName, navigationPath)
    } else {
      // Fallback: navigate by address only (won't update path meaningfully)
      navigateToInstanceByAddress(address, fieldName)
    }
  }

  // Handler for static instance click in ClassExplorer
  const handleStaticInstanceClick = (staticInstance) => {
    if (selectedAssembly && selectedClass) {
      navigateToStaticField(
        selectedAssembly.name,
        selectedClass.name,
        staticInstance.field_name,
        selectedClass.address
      )
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

      {/* Path breadcrumb navigation */}
      {connected && navigationPath.length > 0 && (
        <PathBreadcrumb
          path={navigationPath}
          onNavigate={navigateToPathSegment}
          pathError={pathError}
        />
      )}

      {/* Path resolution error */}
      {pathError && (
        <div className="path-error-banner">
          <strong>Path Resolution Failed:</strong> {pathError.message}
          {pathError.failedSegment && (
            <span className="failed-segment">
              {' '}at "{pathError.failedSegment.type}/{pathError.failedSegment.value}"
            </span>
          )}
          <button onClick={() => setPathError(null)} className="close-btn">×</button>
        </div>
      )}

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
            onSelectAssembly={navigateToAssembly}
            loading={loading}
          />

          {selectedAssembly && (
            <ClassExplorer
              assembly={selectedAssembly}
              selectedClass={selectedClass}
              onSelectClass={(className) => navigateToClass(selectedAssembly.name, className)}
              onSelectInstance={handleStaticInstanceClick}
              loading={loading}
            />
          )}

          {selectedInstance && (
            <InstanceViewer
              instance={selectedInstance}
              onNavigate={handleInstanceFieldNavigate}
              navigationPath={navigationPath}
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
