/**
 * mtga-reader - Node.js bindings for reading Magic: The Gathering Arena memory
 *
 * Cross-platform support:
 * - Windows: Uses Mono backend
 * - macOS: Uses IL2CPP backend
 */

const path = require('path');
const fs = require('fs');

// Try to load the native addon
let nativeBinding = null;

// Possible locations for the .node file
const possiblePaths = [
  // Same directory as index.js
  path.join(__dirname, 'mtga_reader.node'),
  // Platform-specific naming
  path.join(__dirname, `mtga_reader.${process.platform}-${process.arch}.node`),
  // Build output from cargo (release)
  path.join(__dirname, '..', 'target', 'release', 'libmtga_reader.dylib'),
  path.join(__dirname, '..', 'target', 'release', 'mtga_reader.dll'),
];

for (const bindingPath of possiblePaths) {
  if (fs.existsSync(bindingPath)) {
    try {
      nativeBinding = require(bindingPath);
      break;
    } catch (e) {
      // Try next path
    }
  }
}

if (!nativeBinding) {
  throw new Error(
    `Failed to load native binding. Tried:\n${possiblePaths.join('\n')}\n\n` +
    'Please ensure the native addon is built. Run: npm run build'
  );
}

// Export all functions from the native binding
module.exports = {
  /**
   * Check if running with admin/root privileges
   * @returns {boolean}
   */
  isAdmin: nativeBinding.isAdmin,

  /**
   * Check if a process with the given name is running
   * @param {string} processName - Name of the process to find
   * @returns {boolean}
   */
  findProcess: nativeBinding.findProcess,

  /**
   * Initialize the reader for a given process
   * @param {string} processName - Name of the process (e.g., "MTGA")
   * @returns {boolean} - True if initialization succeeded
   * @throws {Error} - If process not found or initialization fails
   */
  init: nativeBinding.init,

  /**
   * Close the reader and release resources
   * @returns {boolean}
   */
  close: nativeBinding.close,

  /**
   * Check if the reader is initialized
   * @returns {boolean}
   */
  isInitialized: nativeBinding.isInitialized,

  /**
   * Get list of loaded assemblies
   * @returns {string[]}
   */
  getAssemblies: nativeBinding.getAssemblies,

  /**
   * Get classes in an assembly
   * @param {string} assemblyName - Name of the assembly
   * @returns {ClassInfo[]}
   */
  getAssemblyClasses: nativeBinding.getAssemblyClasses,

  /**
   * Get detailed class information including fields
   * @param {string} assemblyName - Name of the assembly
   * @param {string} className - Name of the class
   * @returns {ClassDetails}
   */
  getClassDetails: nativeBinding.getClassDetails,

  /**
   * Get instance data at a memory address
   * @param {number} address - Memory address of the instance
   * @returns {InstanceData}
   */
  getInstance: nativeBinding.getInstance,

  /**
   * Get a specific field from an instance
   * @param {number} address - Memory address of the instance
   * @param {string} fieldName - Name of the field
   * @returns {any} - Field value
   */
  getInstanceField: nativeBinding.getInstanceField,

  /**
   * Get a static field from a class
   * @param {number} classAddress - Memory address of the class
   * @param {string} fieldName - Name of the static field
   * @returns {any} - Field value
   */
  getStaticField: nativeBinding.getStaticField,

  /**
   * Read a dictionary at a memory address
   * @param {number} address - Memory address of the dictionary
   * @returns {DictionaryData}
   */
  getDictionary: nativeBinding.getDictionary,

  /**
   * Legacy API: Read data by following a path of field names
   * @param {string} processName - Process name
   * @param {string[]} fields - Array of field names to traverse
   * @returns {any} - The value at the end of the path
   */
  readData: nativeBinding.readData,

  /**
   * Legacy API: Read class at address
   * @param {string} processName - Process name
   * @param {number} address - Memory address
   * @returns {any}
   */
  readClass: nativeBinding.readClass,

  /**
   * Legacy API: Read generic instance at address
   * @param {string} processName - Process name
   * @param {number} address - Memory address
   * @returns {any}
   */
  readGenericInstance: nativeBinding.readGenericInstance,
};

/**
 * @typedef {Object} ClassInfo
 * @property {string} name - Class name
 * @property {string} namespace - Class namespace
 * @property {number} address - Memory address
 * @property {boolean} isStatic - Is static class
 * @property {boolean} isEnum - Is enum type
 */

/**
 * @typedef {Object} FieldInfo
 * @property {string} name - Field name
 * @property {string} typeName - Field type name
 * @property {number} offset - Field offset
 * @property {boolean} isStatic - Is static field
 * @property {boolean} isConst - Is const field
 */

/**
 * @typedef {Object} StaticInstanceInfo
 * @property {string} fieldName - Static field name
 * @property {number} address - Instance address
 */

/**
 * @typedef {Object} ClassDetails
 * @property {string} name - Class name
 * @property {string} namespace - Class namespace
 * @property {number} address - Class address
 * @property {FieldInfo[]} fields - Class fields
 * @property {StaticInstanceInfo[]} staticInstances - Static instances
 */

/**
 * @typedef {Object} InstanceField
 * @property {string} name - Field name
 * @property {string} typeName - Field type
 * @property {boolean} isStatic - Is static
 * @property {any} value - Field value
 */

/**
 * @typedef {Object} InstanceData
 * @property {string} className - Class name
 * @property {string} namespace - Namespace
 * @property {number} address - Instance address
 * @property {InstanceField[]} fields - Instance fields
 */

/**
 * @typedef {Object} DictionaryEntry
 * @property {any} key - Entry key
 * @property {any} value - Entry value
 */

/**
 * @typedef {Object} DictionaryData
 * @property {number} count - Number of entries
 * @property {DictionaryEntry[]} entries - Dictionary entries
 */
