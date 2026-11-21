import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const generatorRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const toolRepositoryRoot = path.resolve(generatorRoot, '../..');
const externalWorkspace = process.env.BEVY_ARCHITECTURE_WORKSPACE;
export const repositoryRoot = externalWorkspace ? fs.realpathSync(externalWorkspace) : toolRepositoryRoot;

export function pathsForRepository(root = repositoryRoot) {
  root = path.resolve(root);
  const selectedProject = root === repositoryRoot;
  const packageRoot = selectedProject ? generatorRoot : path.join(root, 'docs/diagrams');
  const temporaryDirectory = path.join(root, 'tmp/ecs-architecture');
  const generatedDirectory = selectedProject && externalWorkspace ? path.join(temporaryDirectory, 'generated') : path.join(packageRoot, 'generated');
  return {
    repositoryRoot: root, packageRoot, generatedDirectory,
    sourceDirectory: path.join(packageRoot, 'src'),
    testsDirectory: path.join(packageRoot, 'tests'), licensesDirectory: path.join(packageRoot, 'licenses'),
    pagesDirectory: path.join(generatedDirectory, 'pages'), dataDirectory: path.join(generatedDirectory, 'data'),
    viewDataDirectory: path.join(generatedDirectory, 'data/views'),
    objectSvgDirectory: path.join(generatedDirectory, 'svg/object'), scheduleSvgDirectory: path.join(generatedDirectory, 'svg/schedule'),
    legendSvgDirectory: path.join(generatedDirectory, 'svg/legend'), dotDirectory: path.join(generatedDirectory, 'dot'),
    mermaidDirectory: path.join(generatedDirectory, 'mermaid'),
    temporaryDirectory,
  };
}

export function artifactRelativePath(view, kind = 'svg') {
  const filename = kind === 'svg' ? view.filename : kind === 'mermaid' ? view.mermaidFilename : view.dotFilename;
  if (!filename || path.basename(filename) !== filename) throw new Error('Invalid architecture artifact basename');
  return kind === 'svg' ? `svg/${view.aspect === 'schedule' ? 'schedule' : 'object'}/${filename}`
    : `${kind === 'mermaid' ? 'mermaid' : 'dot'}/${filename}`;
}

export function artifactFile(paths, view, kind = 'svg') {
  return path.join(paths.generatedDirectory, artifactRelativePath(view, kind));
}
