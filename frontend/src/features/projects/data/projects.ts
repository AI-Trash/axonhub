import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { graphqlRequest } from '@/gql/graphql';
import { useErrorHandler } from '@/hooks/use-error-handler';

import { Project, ProjectConnection, CreateProjectInput, UpdateProjectInput, projectConnectionSchema, projectSchema } from './schema';
import * as m from '@/paraglide/messages';

// GraphQL queries and mutations
const PROJECTS_QUERY = `
  query GetProjects($first: Int, $after: Cursor, $orderBy: ProjectOrder, $where: ProjectWhereInput) {
    projects(first: $first, after: $after, orderBy: $orderBy, where: $where) {
      edges {
        node {
          id
          createdAt
          updatedAt
          name
          description
          status
        }
        cursor
      }
      pageInfo {
        hasNextPage
        hasPreviousPage
        startCursor
        endCursor
      }
      totalCount
    }
  }
`;

const CREATE_PROJECT_MUTATION = `
  mutation CreateProject($input: CreateProjectInput!) {
    createProject(input: $input) {
      id
      name
      description
      status
      createdAt
      updatedAt
    }
  }
`;

const UPDATE_PROJECT_MUTATION = `
  mutation UpdateProject($id: ID!, $input: UpdateProjectInput!) {
    updateProject(id: $id, input: $input) {
      id
      name
      description
      status
      createdAt
      updatedAt
    }
  }
`;

const UPDATE_PROJECT_STATUS_MUTATION = `
  mutation UpdateProjectStatus($id: ID!, $status: ProjectStatus!) {
    updateProjectStatus(id: $id, status: $status) {
      id
      name
      description
      status
      createdAt
      updatedAt
    }
  }
`;

const MY_PROJECTS_QUERY = `
  query MyProjects {
    myProjects {
        id
        name
        description
        status
        createdAt
        updatedAt
    }
  }
`;

// Query hooks
export function useProjects(
  variables: {
    first?: number;
    after?: string;
    orderBy?: { field: 'CREATED_AT'; direction: 'ASC' | 'DESC' };
    where?: any;
  } = {}
) {
  const { handleError } = useErrorHandler();
  const queryVariables = {
    ...variables,
    orderBy: variables.orderBy || { field: 'CREATED_AT', direction: 'DESC' },
  };

  return useQuery({
    queryKey: ['projects', queryVariables],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ projects: ProjectConnection }>(PROJECTS_QUERY, queryVariables);
        return projectConnectionSchema.parse(data?.projects);
      } catch (error) {
        handleError(error, m["projects.errors.loadProjectsFailed"]());
        throw error;
      }
    },
  });
}

export function useProject(id: string) {
  const { handleError } = useErrorHandler();
  return useQuery({
    queryKey: ['project', id],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ projects: ProjectConnection }>(PROJECTS_QUERY, { where: { id } });
        const project = data.projects.edges[0]?.node;
        if (!project) {
          throw new Error('Project not found');
        }
        return projectSchema.parse(project);
      } catch (error) {
        handleError(error, m["projects.errors.loadProjectDetailFailed"]());
        throw error;
      }
    },
    enabled: !!id,
  });
}

export function useMyProjects() {
  const { handleError } = useErrorHandler();
  return useQuery({
    queryKey: ['myProjects'],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ myProjects: Project[] }>(MY_PROJECTS_QUERY);

        if (!data || !data.myProjects) {
          return [];
        }

        // myProjects 直接返回项目数组，不是 connection 格式
        const projects = data.myProjects.map((project) => projectSchema.parse(project));

        return projects;
      } catch (error) {
        handleError(error, m["projects.errors.loadMyProjectsFailed"]());
        return [];
      }
    },
  });
}

// Mutation hooks
export function useCreateProject() {
  const queryClient = useQueryClient();
  const { handleError } = useErrorHandler();

  return useMutation({
    mutationFn: async (input: CreateProjectInput) => {
      try {
        const data = await graphqlRequest<{ createProject: Project }>(CREATE_PROJECT_MUTATION, { input });
        return projectSchema.parse(data.createProject);
      } catch (error) {
        handleError(error, m["projects.errors.createProjectFailed"]());
        throw error;
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['myProjects'] });
      toast.success(m["common.success.projectCreated"]());
    },
  });
}

export function useUpdateProject() {
  const queryClient = useQueryClient();
  const { handleError } = useErrorHandler();

  return useMutation({
    mutationFn: async ({ id, input }: { id: string; input: UpdateProjectInput }) => {
      try {
        const data = await graphqlRequest<{ updateProject: Project }>(UPDATE_PROJECT_MUTATION, { id, input });
        return projectSchema.parse(data.updateProject);
      } catch (error) {
        handleError(error, m["projects.errors.updateProjectFailed"]());
        throw error;
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project'] });
      queryClient.invalidateQueries({ queryKey: ['myProjects'] });
      toast.success(m["common.success.projectUpdated"]());
    },
  });
}

export function useArchiveProject() {
  const queryClient = useQueryClient();
  const { handleError } = useErrorHandler();

  return useMutation({
    mutationFn: async (id: string) => {
      try {
        const data = await graphqlRequest<{ updateProjectStatus: Project }>(UPDATE_PROJECT_STATUS_MUTATION, {
          id,
          status: 'archived',
        });
        return projectSchema.parse(data.updateProjectStatus);
      } catch (error) {
        handleError(error, m["projects.errors.archiveProjectFailed"]());
        throw error;
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project'] });
      queryClient.invalidateQueries({ queryKey: ['myProjects'] });
      toast.success(m["common.success.projectArchived"]());
    },
  });
}

export function useActivateProject() {
  const queryClient = useQueryClient();
  const { handleError } = useErrorHandler();

  return useMutation({
    mutationFn: async (id: string) => {
      try {
        const data = await graphqlRequest<{ updateProjectStatus: Project }>(UPDATE_PROJECT_STATUS_MUTATION, {
          id,
          status: 'active',
        });
        return projectSchema.parse(data.updateProjectStatus);
      } catch (error) {
        handleError(error, m["projects.errors.activateProjectFailed"]());
        throw error;
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project'] });
      queryClient.invalidateQueries({ queryKey: ['myProjects'] });
      toast.success(m["common.success.projectActivated"]());
    },
  });
}
