/**
 * mtga-reader - Node.js bindings for reading Magic: The Gathering Arena memory
 */

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
  value: unknown;
}

export interface InstanceData {
  className: string;
  namespace: string;
  address: number;
  fields: InstanceField[];
}

export interface DictionaryEntry {
  key: unknown;
  value: unknown;
}

export interface DictionaryData {
  count: number;
  entries: DictionaryEntry[];
}

/**
 * Check if running with admin/root privileges
 */
export function isAdmin(): boolean;

/**
 * Check if a process with the given name is running
 */
export function findProcess(processName: string): boolean;

/**
 * Initialize the reader for a given process
 * @throws Error if process not found or initialization fails
 */
export function init(processName: string): boolean;

/**
 * Close the reader and release resources
 */
export function close(): boolean;

/**
 * Check if the reader is initialized
 */
export function isInitialized(): boolean;

/**
 * Get list of loaded assemblies
 */
export function getAssemblies(): string[];

/**
 * Get classes in an assembly
 */
export function getAssemblyClasses(assemblyName: string): ClassInfo[];

/**
 * Get detailed class information including fields
 */
export function getClassDetails(assemblyName: string, className: string): ClassDetails;

/**
 * Get instance data at a memory address
 */
export function getInstance(address: number): InstanceData;

/**
 * Get a specific field from an instance
 */
export function getInstanceField(address: number, fieldName: string): unknown;

/**
 * Get a static field from a class
 */
export function getStaticField(classAddress: number, fieldName: string): unknown;

/**
 * Read a dictionary at a memory address
 */
export function getDictionary(address: number): DictionaryData;

/**
 * Legacy API: Read data by following a path of field names
 * Windows: WrapperController path (WrapperController.Instance.InventoryManager...)
 * macOS: PAPA path (PAPA._InventoryManager.GetPlayerCardsNoLock...)
 */
export function readData(processName: string, fields: string[]): unknown;

/**
 * Legacy API: Read class at address
 */
export function readClass(processName: string, address: number): unknown;

/**
 * Legacy API: Read generic instance at address
 */
export function readGenericInstance(processName: string, address: number): unknown;
