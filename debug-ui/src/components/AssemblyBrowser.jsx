import React, { useState } from 'react'
import './AssemblyBrowser.css'

function AssemblyBrowser({ assemblies, selectedAssembly, onSelectAssembly, loading }) {
  const [searchTerm, setSearchTerm] = useState('')

  const filteredAssemblies = assemblies.filter(asm =>
    asm.toLowerCase().includes(searchTerm.toLowerCase())
  )

  return (
    <div className="assembly-browser panel">
      <div className="panel-header">
        <h2>Assemblies</h2>
        <div className="count-badge">{assemblies.length}</div>
      </div>

      <div className="search-box">
        <input
          type="text"
          placeholder="Search assemblies..."
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          className="search-input"
        />
      </div>

      <div className="assembly-list">
        {filteredAssemblies.map((assembly) => (
          <div
            key={assembly}
            className={`assembly-item ${selectedAssembly?.name === assembly ? 'selected' : ''}`}
            onClick={() => !loading && onSelectAssembly(assembly)}
          >
            <span className="assembly-icon">📦</span>
            <span className="assembly-name">{assembly}</span>
          </div>
        ))}
      </div>

      {filteredAssemblies.length === 0 && (
        <div className="empty-message">
          No assemblies found
        </div>
      )}
    </div>
  )
}

export default AssemblyBrowser
