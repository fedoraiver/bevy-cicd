import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { generatorRoot, repositoryRoot } from './paths.mjs';

export { repositoryRoot };
export const maintenanceScripts = 'docs/diagrams/src';

// 维护入口复用图包的 npm 环境，仅在实际使用时加载依赖。
const diagramRequire = createRequire(path.join(generatorRoot, 'package.json'));
export const importDiagramDependency = name => import(pathToFileURL(diagramRequire.resolve(name)).href);
