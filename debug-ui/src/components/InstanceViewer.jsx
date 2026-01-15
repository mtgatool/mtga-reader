import React, { useState } from 'react'
import './InstanceViewer.css'

function InstanceViewer({ instance, onNavigate, loading }) {
  const [expandedNodes, setExpandedNodes] = useState(new Set())
  const [fieldValues, setFieldValues] = useState(new Map())
  const [dictionaryData, setDictionaryData] = useState(new Map())

  const toggleNode = (path) => {
    const newExpanded = new Set(expandedNodes)
    if (newExpanded.has(path)) {
      newExpanded.delete(path)
    } else {
      newExpanded.add(path)
    }
    setExpandedNodes(newExpanded)
  }

  const readInstanceFieldValue = async (fieldName, instanceAddress) => {
    try {
      const response = await fetch(`http://localhost:8080/instance/0x${instanceAddress.toString(16)}/field/${encodeURIComponent(fieldName)}`)
      const data = await response.json()

      const newFieldValues = new Map(fieldValues)
      newFieldValues.set(fieldName, data)
      setFieldValues(newFieldValues)
    } catch (error) {
      console.error('Error reading instance field value:', error)
      alert(`Failed to read field value: ${error.message}`)
    }
  }

  const readDictionary = async (dictAddress) => {
    try {
      const response = await fetch(`http://localhost:8080/dictionary/0x${dictAddress.toString(16)}`)
      const data = await response.json()

      const newDictData = new Map(dictionaryData)
      newDictData.set(dictAddress, data)
      setDictionaryData(newDictData)
    } catch (error) {
      console.error('Error reading dictionary:', error)
      alert(`Failed to read dictionary: ${error.message}`)
    }
  }

  const renderValue = (value, path = '') => {
    if (value === null || value === undefined) {
      return <span className="value-null">null</span>
    }

    if (typeof value === 'boolean') {
      return <span className="value-boolean">{value.toString()}</span>
    }

    if (typeof value === 'number') {
      return <span className="value-number">{value}</span>
    }

    if (typeof value === 'string') {
      return <span className="value-string">"{value}"</span>
    }

    if (value.type === 'pointer') {
      return (
        <span
          className="value-pointer"
          onClick={() => value.address && onNavigate(value.address)}
          title="Click to navigate"
        >
          → 0x{value.address?.toString(16)}
          {value.class_name && <span className="pointer-type"> ({value.class_name})</span>}
        </span>
      )
    }

    if (Array.isArray(value)) {
      const isExpanded = expandedNodes.has(path)
      return (
        <div className="value-array">
          <div className="array-header" onClick={() => toggleNode(path)}>
            <span className="expand-icon">{isExpanded ? '▼' : '▶'}</span>
            <span className="array-label">Array[{value.length}]</span>
          </div>
          {isExpanded && (
            <div className="array-content">
              {value.map((item, index) => (
                <div key={index} className="array-item">
                  <span className="item-index">[{index}]</span>
                  {renderValue(item, `${path}[${index}]`)}
                </div>
              ))}
            </div>
          )}
        </div>
      )
    }

    if (typeof value === 'object') {
      const isExpanded = expandedNodes.has(path)
      const keys = Object.keys(value)

      return (
        <div className="value-object">
          <div className="object-header" onClick={() => toggleNode(path)}>
            <span className="expand-icon">{isExpanded ? '▼' : '▶'}</span>
            <span className="object-label">Object ({keys.length} properties)</span>
          </div>
          {isExpanded && (
            <div className="object-content">
              {keys.map((key) => (
                <div key={key} className="object-property">
                  <span className="property-key">{key}:</span>
                  {renderValue(value[key], `${path}.${key}`)}
                </div>
              ))}
            </div>
          )}
        </div>
      )
    }

    return <span className="value-unknown">{String(value)}</span>
  }

  return (
    <div className="instance-viewer panel">
      <div className="panel-header">
        <h2>Instance Viewer</h2>
      </div>

      <div className="instance-content">
        <div className="instance-header-info">
          <div className="instance-title">
            <span className="instance-class">{instance.class_name}</span>
            {instance.namespace && (
              <span className="instance-namespace">{instance.namespace}</span>
            )}
          </div>
          <code className="instance-addr">0x{instance.address?.toString(16)}</code>
        </div>

        {instance.fields && instance.fields.length > 0 && (
          <div className="fields-tree">
            {instance.fields.map((field) => {
              const storedValue = fieldValues.get(field.name)
              const isDictionary = field.type && (field.type.includes('Dictionary') || field.type.includes('IDictionary'))
              const isPointer = field.value && typeof field.value === 'object' && field.value.type === 'pointer' && field.value.address !== 0
              const isPrimitive = field.type && (
                field.type.includes('Int32') || field.type.includes('Int64') ||
                field.type.includes('UInt32') || field.type.includes('UInt64') ||
                field.type.includes('Boolean') || field.type.includes('String')
              )

              return (
                <div key={field.name} className="tree-node">
                  <div className="tree-node-header">
                    <span className="field-icon">
                      {field.is_static ? '🔹' : '•'}
                    </span>
                    <span className="field-name-label">{field.name}</span>
                    <span className="field-type-label">{field.type}</span>
                    {!field.is_static && (isPrimitive || isPointer) && !storedValue && (
                      <button
                        className="read-value-btn-inline"
                        onClick={() => readInstanceFieldValue(field.name, instance.address)}
                        title="Read field value"
                      >
                        📖
                      </button>
                    )}
                  </div>
                  <div className="tree-node-value">
                    {storedValue ? (
                      <div className="resolved-value">
                        {storedValue.type === 'primitive' ? (
                          <span style={{
                            color: storedValue.value_type === 'boolean' ? '#569cd6' : '#b5cea8',
                            fontFamily: 'Courier New, monospace'
                          }}>
                            {storedValue.value_type === 'boolean'
                              ? (storedValue.value ? 'true' : 'false')
                              : storedValue.value}
                          </span>
                        ) : storedValue.type === 'pointer' && storedValue.address !== 0 ? (
                          <div>
                            <span
                              className="value-pointer"
                              onClick={() => onNavigate(storedValue.address)}
                              title="Click to navigate"
                            >
                              → 0x{storedValue.address.toString(16)}
                              {storedValue.class_name && (
                                <span className="pointer-type"> ({storedValue.class_name})</span>
                              )}
                            </span>
                            {isDictionary && (
                              <button
                                className="read-dict-btn"
                                onClick={() => readDictionary(storedValue.address)}
                                style={{ marginLeft: '0.5rem' }}
                              >
                                Read Dictionary
                              </button>
                            )}
                            {dictionaryData.has(storedValue.address) && (
                              <div className="dictionary-preview" style={{ marginTop: '0.5rem' }}>
                                <strong>Dictionary ({dictionaryData.get(storedValue.address).count} entries):</strong>
                                <div style={{ maxHeight: '200px', overflow: 'auto', marginTop: '0.25rem' }}>
                                  {dictionaryData.get(storedValue.address).entries.slice(0, 100).map((entry, idx) => (
                                    <div key={idx} style={{ fontSize: '0.85rem', padding: '2px 0' }}>
                                      <span style={{ color: '#b5cea8' }}>{entry.key}</span>
                                      {' → '}
                                      <span style={{ color: '#b5cea8' }}>{entry.value}</span>
                                    </div>
                                  ))}
                                  {dictionaryData.get(storedValue.address).entries.length > 100 && (
                                    <div style={{ color: '#858585', fontSize: '0.85rem' }}>
                                      ... and {dictionaryData.get(storedValue.address).entries.length - 100} more
                                    </div>
                                  )}
                                </div>
                              </div>
                            )}
                          </div>
                        ) : (
                          <span className="value-null">null</span>
                        )}
                      </div>
                    ) : (
                      renderValue(field.value, field.name)
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        )}

        {(!instance.fields || instance.fields.length === 0) && (
          <div className="empty-message">
            <p>No field data available</p>
            <div style={{ marginTop: '1rem' }}>
              <p style={{ color: '#858585', fontSize: '0.875rem', marginBottom: '0.5rem' }}>
                This class may inherit from Dictionary. Try reading it as a dictionary:
              </p>
              <button
                className="read-dict-btn"
                onClick={() => readDictionary(instance.address)}
              >
                Read as Dictionary
              </button>
              {dictionaryData.has(instance.address) && (
                <div className="dictionary-preview" style={{ marginTop: '1rem' }}>
                  <strong>Dictionary ({dictionaryData.get(instance.address).count} entries):</strong>
                  <div style={{ maxHeight: '400px', overflow: 'auto', marginTop: '0.5rem' }}>
                    {dictionaryData.get(instance.address).entries.slice(0, 100).map((entry, idx) => (
                      <div key={idx} style={{ fontSize: '0.85rem', padding: '2px 0' }}>
                        <span style={{ color: '#b5cea8' }}>{entry.key}</span>
                        {' → '}
                        <span style={{ color: '#b5cea8' }}>{entry.value}</span>
                      </div>
                    ))}
                    {dictionaryData.get(instance.address).entries.length > 100 && (
                      <div style={{ color: '#858585', fontSize: '0.85rem', marginTop: '0.5rem' }}>
                        ... and {dictionaryData.get(instance.address).entries.length - 100} more entries
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

export default InstanceViewer
