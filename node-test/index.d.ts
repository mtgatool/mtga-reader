/* Auto-generated TypeScript definitions for mtga-reader-node */

export interface ClassInfo {
  name: string;
  namespace: string;
  address: number;
  isStatic: boolean;
  isEnum: boolean;
}

export interface FieldInfo {
  name: string;
  typeName: string;
  offset: number;
  isStatic: boolean;
  isConst: boolean;
}

export interface StaticInstanceInfo {
  fieldName: string;
  address: number;
}

export interface ClassDetails {
  name: string;
  namespace: string;
  address: number;
  fields: FieldInfo[];
  staticInstances: StaticInstanceInfo[];
}

export interface InstanceField {
  name: string;
  typeName: string;
  isStatic: boolean;
  value: any;
}

export interface InstanceData {
  className: string;
  namespace: string;
  address: number;
  fields: InstanceField[];
}

export interface DictionaryEntry {
  key: any;
  value: any;
}

export interface DictionaryData {
  count: number;
  entries: DictionaryEntry[];
}

// Utility functions

/**
 * Check if the current process has administrator privileges
 */
export function isAdmin(): boolean;

/**
 * Find a process by name and return true if found
 */
export function findProcess(processName: string): boolean;

/**
 * Initialize connection to the target process
 * Must be called before using any other reader functions
 */
export function init(processName: string): boolean;

/**
 * Close the connection to the target process
 */
export function close(): boolean;

/**
 * Check if the reader is initialized
 */
export function isInitialized(): boolean;

// Assembly functions

/**
 * Get all loaded assembly names
 */
export function getAssemblies(): string[];

/**
 * Get all classes in an assembly
 */
export function getAssemblyClasses(assemblyName: string): ClassInfo[];

/**
 * Get detailed information about a class
 */
export function getClassDetails(assemblyName: string, className: string): ClassDetails;

// Instance reading functions

/**
 * Read an instance at a given memory address
 */
export function getInstance(address: number): InstanceData;

/**
 * Read a specific field from an instance
 */
export function getInstanceField(address: number, fieldName: string): any;

/**
 * Read a static field from a class
 */
export function getStaticField(classAddress: number, fieldName: string): any;

// Dictionary reading

/**
 * Read a dictionary at a given memory address
 */
export function getDictionary(address: number): DictionaryData;

// High-level data reading

/**
 * Read nested data by traversing a path of field names
 * The first element is the root class name, subsequent elements are field names
 */
export function readData(processName: string, fields: string[]): any;

/**
 * Read a managed class at a given address
 */
export function readClass(processName: string, address: number): any;

/**
 * Read a generic instance at a given address
 */
export function readGenericInstance(processName: string, address: number): any;
