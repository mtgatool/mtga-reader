import React, { useState } from 'react'
import './ClassExplorer.css'

function ClassExplorer({ assembly, selectedClass, onSelectClass, onSelectInstance, loading }) {
  const [searchTerm, setSearchTerm] = useState('')
  const [expandedFields, setExpandedFields] = useState(new Set())
  const [fieldValues, setFieldValues] = useState(new Map())

  const filteredClasses = (assembly?.classes || []).filter(cls =>
    cls.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
    cls.namespace?.toLowerCase().includes(searchTerm.toLowerCase())
  )

  const toggleField = (fieldName) => {
    const newExpanded = new Set(expandedFields)
    if (newExpanded.has(fieldName)) {
      newExpanded.delete(fieldName)
    } else {
      newExpanded.add(fieldName)
    }
    setExpandedFields(newExpanded)
  }

  const readFieldValue = async (field, classAddress) => {
    try {
      // Use the new endpoint to read static field values
      const response = await fetch(`http://localhost:8080/class/0x${classAddress.toString(16)}/field/${encodeURIComponent(field.name)}`)
      const data = await response.json()

      const newFieldValues = new Map(fieldValues)
      newFieldValues.set(field.name, data)
      setFieldValues(newFieldValues)
    } catch (error) {
      console.error('Error reading field value:', error)
      alert(`Failed to read field value: ${error.message}`)
    }
  }

  return (
    <div className="class-explorer panel">
      <div className="panel-header">
        <h2>Classes</h2>
        <div className="count-badge">{assembly?.classes?.length || 0}</div>
      </div>

      <div className="search-box">
        <input
          type="text"
          placeholder="Search classes..."
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          className="search-input"
        />
      </div>

      <div className="split-view">
        <div className="class-list">
          {filteredClasses.map((cls) => (
            <div
              key={cls.address}
              className={`class-item ${selectedClass?.address === cls.address ? 'selected' : ''}`}
              onClick={() => !loading && onSelectClass(cls.name)}
            >
              <div className="class-header">
                <span className="class-icon">
                  {cls.is_static ? '🔷' : cls.is_enum ? '🔢' : '📄'}
                </span>
                <span className="class-name">{cls.name}</span>
              </div>
              {cls.namespace && (
                <div className="class-namespace">{cls.namespace}</div>
              )}
            </div>
          ))}

          {filteredClasses.length === 0 && (
            <div className="empty-message">
              No classes found
            </div>
          )}
        </div>

        {selectedClass && (
          <div className="class-details">
            <div className="details-header">
              <h3>{selectedClass.name}</h3>
              {selectedClass.namespace && (
                <div className="namespace-tag">{selectedClass.namespace}</div>
              )}
            </div>

            <div className="class-info">
              <div className="info-row">
                <span className="info-label">Address:</span>
                <code className="info-value">0x{selectedClass.address?.toString(16)}</code>
              </div>
              <div className="info-row">
                <span className="info-label">Fields:</span>
                <span className="info-value">{selectedClass.fields?.length || 0}</span>
              </div>
              {selectedClass.parent && (
                <div className="info-row">
                  <span className="info-label">Parent:</span>
                  <span className="info-value">{selectedClass.parent}</span>
                </div>
              )}
            </div>

            {selectedClass.static_instances && selectedClass.static_instances.length > 0 && (
              <div className="section">
                <h4>Static Instances</h4>
                <div className="instance-list">
                  {selectedClass.static_instances.map((inst) => (
                    <div
                      key={inst.field_name}
                      className="instance-item"
                      onClick={() => inst.address && onSelectInstance(inst.address)}
                    >
                      <span className="instance-icon">🎯</span>
                      <div className="instance-info">
                        <div className="instance-name">{inst.field_name}</div>
                        <code className="instance-address">0x{inst.address?.toString(16)}</code>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {selectedClass.fields && selectedClass.fields.length > 0 && (
              <div className="section">
                <h4>Fields</h4>
                <div className="field-list">
                  {selectedClass.fields.map((field) => {
                    const fieldValue = fieldValues.get(field.name)

                    return (
                      <div
                        key={field.name}
                        className={`field-item ${expandedFields.has(field.name) ? 'expanded' : ''}`}
                      >
                        <div className="field-header" onClick={() => toggleField(field.name)}>
                          <span className="field-icon">
                            {field.is_static ? '🔹' : '•'}
                          </span>
                          <span className="field-name">{field.name}</span>
                          <span className="field-type">{field.type}</span>
                          {field.is_static && !field.is_const && (
                            <span className="pointer-indicator">📖</span>
                          )}
                        </div>
                        {expandedFields.has(field.name) && (
                          <div className="field-details">
                            <div className="field-detail-row">
                              <span>Offset:</span>
                              <code>0x{field.offset?.toString(16)}</code>
                            </div>
                            <div className="field-detail-row">
                              <span>Static:</span>
                              <span>{field.is_static ? 'Yes' : 'No'}</span>
                            </div>
                            {field.is_const && (
                              <div className="field-detail-row">
                                <span>Const:</span>
                                <span>Yes</span>
                              </div>
                            )}
                            {field.is_static && !field.is_const && (
                              <div className="field-detail-row">
                                <button
                                  className="read-value-btn"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    readFieldValue(field, selectedClass.address)
                                  }}
                                >
                                  Read Value
                                </button>
                              </div>
                            )}
                            {fieldValue && (
                              <div className="field-value-preview">
                                <strong>Value:</strong>
                                <div className="value-info">
                                  {fieldValue.type === 'null' || fieldValue === null ? (
                                    <span style={{ color: '#569cd6', fontStyle: 'italic' }}>null</span>
                                  ) : fieldValue.type === 'primitive' ? (
                                    <div>
                                      <span style={{
                                        color: fieldValue.value_type === 'boolean' ? '#569cd6' : '#b5cea8',
                                        fontFamily: 'Courier New, monospace'
                                      }}>
                                        {fieldValue.value_type === 'boolean'
                                          ? (fieldValue.value ? 'true' : 'false')
                                          : fieldValue.value}
                                      </span>
                                      <span style={{ color: '#858585', marginLeft: '0.5rem', fontSize: '0.75rem' }}>
                                        ({fieldValue.value_type})
                                      </span>
                                    </div>
                                  ) : fieldValue.type === 'pointer' && fieldValue.address !== 0 ? (
                                    <div
                                      className="value-address clickable"
                                      onClick={(e) => {
                                        e.stopPropagation()
                                        onSelectInstance(fieldValue.address)
                                      }}
                                    >
                                      → 0x{fieldValue.address.toString(16)}
                                      {fieldValue.class_name && (
                                        <span style={{ color: '#858585', marginLeft: '0.5rem' }}>
                                          ({fieldValue.class_name})
                                        </span>
                                      )}
                                      <span className="click-hint"> (click to inspect)</span>
                                    </div>
                                  ) : (
                                    <span style={{ color: '#569cd6', fontStyle: 'italic' }}>null (0x0)</span>
                                  )}
                                </div>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

export default ClassExplorer
